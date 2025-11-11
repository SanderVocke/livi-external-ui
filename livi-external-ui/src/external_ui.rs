use libloading::{Library, Symbol};
use lilv::node::Node;
use lv2_external_ui_sys::{LV2_EXTERNAL_UI__Host, LV2_External_UI_Host, LV2_External_UI_Widget};
use lv2_sys::{
    LV2_Feature, LV2_INSTANCE_ACCESS_URI, LV2UI_Controller, LV2UI_Descriptor, LV2UI_DescriptorFunction, LV2UI_Handle, LV2UI_Widget
};
use std::{
    ffi::{CStr, CString, c_void},
    fmt,
    sync::mpsc::{Receiver, Sender},
};

pub struct BinaryPath {
    pub _hostname: String,
    pub path: String,
}

pub struct ExternalUIWorld {
    external_ui_uri: Node,
}

impl ExternalUIWorld {
    pub fn new(world: &lilv::World) -> Self {
        ExternalUIWorld {
            external_ui_uri: world.new_uri("http://kxstudio.sf.net/ns/lv2ext/external-ui#Widget"),
        }
    }
}

/// An external UI descriptor that can be used to find an external
/// UI binary.
pub struct ExternalUI {
    pub binary: BinaryPath,
    pub bundle: BinaryPath,
    pub raw: lilv::ui::UI,
}

#[derive(Debug)]
pub enum LiviExternalUIError {
    IsNotExternalUI,
    FailedToInspect,
    InstantiateError(String),
    LoadLibraryError(String, libloading::Error),
    LoadDescriptorError,
}

impl fmt::Display for LiviExternalUIError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            LiviExternalUIError::IsNotExternalUI => write!(f, "UI is not an external UI")?,
            LiviExternalUIError::FailedToInspect => write!(f, "Failed to inspect UI information")?,
            LiviExternalUIError::LoadLibraryError(path, e) => write!(
                f,
                "Failed to load descriptor from external UI library {path}: {e}"
            )?,
            LiviExternalUIError::InstantiateError(e) => write!(f, "Failed to instantiate UI: {e}")?,
            LiviExternalUIError::LoadDescriptorError => write!(f, "Failed to load UI descriptor")?,
        }
        Ok(())
    }
}

impl std::error::Error for LiviExternalUIError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            LiviExternalUIError::LoadLibraryError(_, e) => Some(e),
            _ => None,
        }
    }
}

impl ExternalUI {
    pub fn is_external_ui(world: &ExternalUIWorld, ui: &lilv::ui::UI) -> bool {
        ui.is_a(&world.external_ui_uri)
    }

    pub fn from_ui(world: &ExternalUIWorld, ui: lilv::ui::UI) -> Result<Self, LiviExternalUIError> {
        match Self::is_external_ui(world, &ui) {
            true => {
                let binary = ui
                    .binary_uri()
                    .map(|node| node.path())
                    .flatten()
                    .map(|(hostname, path)| BinaryPath {
                        _hostname: hostname,
                        path: path,
                    })
                    .ok_or(LiviExternalUIError::FailedToInspect)?;
                let bundle = ui
                    .bundle_uri()
                    .map(|node| node.path())
                    .flatten()
                    .map(|(hostname, path)| BinaryPath {
                        _hostname: hostname,
                        path: path,
                    })
                    .ok_or(LiviExternalUIError::FailedToInspect)?;
                Ok(ExternalUI {
                    binary: binary,
                    bundle: bundle,
                    raw: ui,
                })
            }
            false => Err(LiviExternalUIError::IsNotExternalUI),
        }
    }

    pub fn load(&self) -> Result<ExternalUILibrary, LiviExternalUIError> {
        ExternalUILibrary::load(self)
    }
}

// TODO: ignoring the hostname part for now
fn load_library(path: &BinaryPath) -> Result<Library, LiviExternalUIError> {
    unsafe {
        Library::new(&path.path)
            .map_err(|e| LiviExternalUIError::LoadLibraryError(path.path.clone(), e))
    }
}

pub struct ExternalUILibrary {
    pub library: Library,
    pub descriptor: LV2UI_Descriptor,
}

impl ExternalUILibrary {
    fn load(ui: &ExternalUI) -> Result<Self, LiviExternalUIError> {
        let lib = load_library(&ui.binary)?;
        let descriptor_fn = unsafe {
            lib.get::<Symbol<LV2UI_DescriptorFunction>>(b"lv2ui_descriptor\0")
                .map_err(|e| LiviExternalUIError::LoadLibraryError(ui.binary.path.clone(), e))?
        }
        .ok_or(LiviExternalUIError::LoadDescriptorError)?;
        let descriptor = unsafe { descriptor_fn(0) };

        Ok(ExternalUILibrary {
            library: lib,
            descriptor: unsafe { descriptor.as_ref() }
                .ok_or(LiviExternalUIError::LoadDescriptorError)?
                .clone(),
        })
    }

    pub fn instantiate(
        &self,
        ui: &ExternalUI,
        plugin_instance: &livi::Instance,
    ) -> Result<(ExternalUIInstance, Box<ExternalUIInstanceRunner>), LiviExternalUIError> {
        instantiate_external_ui(ui, &self, plugin_instance)
    }
}

pub struct ExternalUIControlMessage {
    pub port_index: u32,
    pub buffer_size: u32,
    pub port_protocol: u32,
    pub buffer: *const c_void,
}

struct ExternalUIControlSender {
    sender: Sender<ExternalUIControlMessage>,
}

type ExternalUIReceiver = Receiver<ExternalUIControlMessage>;

pub struct ExternalUIInstance {
    _host: Box<LV2_External_UI_Host>,
    _ui_handle: LV2UI_Handle,
    control_receiver: ExternalUIReceiver,
}

pub struct ExternalUIInstanceRunner {
    widget: *mut LV2_External_UI_Widget,
    control_sender: ExternalUIControlSender,
}

unsafe impl Send for ExternalUIInstanceRunner {}

extern "C" fn static_ui_write_fn(
    controller : LV2UI_Controller,
    port_index: u32,
    buffer_size: u32,
    port_protocol: u32,
    buffer: *const c_void,
) {
    let sender = unsafe { &mut *(controller as *mut ExternalUIControlSender) }; 
    if let Err(e) = sender.sender.send(ExternalUIControlMessage {
        port_index: port_index,
        buffer_size: buffer_size,
        port_protocol: port_protocol,
        buffer: buffer,
    }) {
        eprintln!("Failed to send control message from UI: {e}");
    }
}

fn instantiate_external_ui(
    ui: &ExternalUI,
    lib: &ExternalUILibrary,
    plugin_instance: &livi::Instance,
) -> Result<(ExternalUIInstance, Box<ExternalUIInstanceRunner>), LiviExternalUIError> {
    let mut widget: LV2UI_Widget = std::ptr::null_mut();
    let mut host = Box::new(LV2_External_UI_Host {
        ui_closed: None,
        plugin_human_id: CStr::from_bytes_with_nul(b"test\0")
            .map_err(|e| {
                LiviExternalUIError::InstantiateError(format!("Could not create plugin ID: {e}"))
            })?
            .as_ptr(),
    });
    let instance_handle = plugin_instance.raw().instance().handle();
    let instance_access_feature = LV2_Feature {
        URI: LV2_INSTANCE_ACCESS_URI as *const u8 as *const i8,
        data: instance_handle as *mut std::ffi::c_void,
    };
    let ui_host_feature = LV2_Feature {
        URI: LV2_EXTERNAL_UI__Host as *const u8 as *const i8,
        data: host.as_mut() as *mut LV2_External_UI_Host as *mut std::ffi::c_void,
    };
    let instantiate_fn =
        lib.descriptor
            .instantiate
            .ok_or(LiviExternalUIError::InstantiateError(String::from(
                "No instantiation function available",
            )))?;
    let plugin_uri =
        plugin_instance
            .raw()
            .instance()
            .uri()
            .ok_or(LiviExternalUIError::InstantiateError(String::from(
                "Could not get plugin URI",
            )))?;
    let features: [*const LV2_Feature; _] =
        [&instance_access_feature, &ui_host_feature, std::ptr::null()];
    let (sender, receiver) = std::sync::mpsc::channel();
    let mut runner = Box::new(ExternalUIInstanceRunner {
        widget: std::ptr::null_mut(),
        control_sender: ExternalUIControlSender { sender: sender },
    });
    let instance = unsafe {
        instantiate_fn(
            &lib.descriptor as *const LV2UI_Descriptor,
            CString::new(plugin_uri)
                .map_err(|_| {
                    LiviExternalUIError::InstantiateError(String::from(
                        "C string construction error",
                    ))
                })?
                .as_ptr(),
            CString::new(ui.bundle.path.clone())
                .map_err(|_| {
                    LiviExternalUIError::InstantiateError(String::from(
                        "C string construction error",
                    ))
                })?
                .as_ptr(),
            Some(static_ui_write_fn),
            &mut runner.as_mut().control_sender
                as *mut ExternalUIControlSender as *mut std::ffi::c_void,
            &mut widget as *mut LV2UI_Widget as *mut *mut std::ffi::c_void,
            features.as_ptr() as *const _,
        )
    };
    runner.widget = widget as *mut LV2_External_UI_Widget;
    Ok((
        ExternalUIInstance {
            _host: host,
            _ui_handle: instance,
            control_receiver: receiver,
        },
        runner,
    ))
}

impl ExternalUIInstanceRunner {
    pub fn show(&self) -> Result<(), LiviExternalUIError> {
        let widget: &mut LV2_External_UI_Widget = unsafe { &mut *self.widget };
        match widget.show {
            Some(f) => {
                unsafe { f(self.widget) };
                Ok(())
            }
            None => Err(LiviExternalUIError::InstantiateError(String::from(
                "No show function available",
            ))),
        }
    }

    pub fn hide(&self) -> Result<(), LiviExternalUIError> {
        let widget: &mut LV2_External_UI_Widget = unsafe { &mut *self.widget };
        match widget.hide {
            Some(f) => {
                unsafe { f(self.widget) };
                Ok(())
            }
            None => Err(LiviExternalUIError::InstantiateError(String::from(
                "No show function available",
            ))),
        }
    }

    pub fn run(&self) -> Result<(), LiviExternalUIError> {
        let widget: &mut LV2_External_UI_Widget = unsafe { &mut *self.widget };
        match widget.run {
            Some(f) => {
                unsafe { f(self.widget) };
                Ok(())
            }
            None => Err(LiviExternalUIError::InstantiateError(String::from(
                "No show function available",
            ))),
        }
    }
}

impl ExternalUIInstance {
    pub fn pending_ui_control_msgs(
        &self,
    ) -> Result<impl Iterator<Item = ExternalUIControlMessage>, LiviExternalUIError> {
        Ok(self.control_receiver.try_iter())
    }
}
