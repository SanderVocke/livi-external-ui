use livi_external_ui::external_ui::ExternalUIInstance;
use std::{thread, time::Duration};

fn main_impl() -> Result<(), Box<dyn std::error::Error>> {
    let world = livi::World::new();
    const SAMPLE_RATE: f64 = 44100.0;
    let features = world.build_features(livi::FeaturesBuilder {
        min_block_length: 1,
        max_block_length: 4096,
    });
    let plugin = world
        // This is the URI for mda EPiano. You can use the `lv2ls` command line
        // utility to see all available LV2 plugins.
        .plugin_by_uri("http://kxstudio.sf.net/carla/plugins/carlarack")
        .expect("Plugin not found.");

    let mut instance = unsafe {
        plugin
            .instantiate(features.clone(), SAMPLE_RATE)
            .expect("Could not instantiate plugin.")
    };

    // Where midi events will be read from.
    let input = {
        let mut s = livi::event::LV2AtomSequence::new(&features, 1024);
        let play_note_data = [0x90, 0x40, 0x7f];
        s.push_midi_event::<3>(1, features.midi_urid(), &play_note_data)
            .unwrap();
        s
    };
    let mut output = livi::event::LV2AtomSequence::new(&features, 1024);

    // This is where the audio data will be stored.
    let mut outputs: [Vec<f32>; 2] = [
        vec![0.0; features.max_block_length()], // For mda EPiano, this is the left channel.
        vec![0.0; features.max_block_length()], // For mda EPiano, this is the right channel.
    ];
    let mut inputs: [Vec<f32>; 2] = [
        vec![0.0; features.max_block_length()], // For mda EPiano, this is the left channel.
        vec![0.0; features.max_block_length()], // For mda EPiano, this is the right channel.
    ];

    let uis = &livi_external_ui::ui::plugin_uis(&world, &plugin)?;
    let mut ui_instance: Option<ExternalUIInstance> = None;

    if uis.len() > 0 {
        if let livi_external_ui::ui::UI::External(ui) = &uis[0] {
            println!("Loading plugin: {}", ui.binary.path);
            let ui_loaded = Some(ui.load()?);
            println!("Instantiating UI.");
            let (inst, runner) = ui_loaded.unwrap().instantiate(&ui, &instance)?;
            ui_instance = Some(inst);

            // Start a UI thread to update the UI repeatedly.
            let _ = thread::spawn(move || {
                if let Err(e) = || -> Result<(), Box<dyn std::error::Error>> {
                    runner.show()?;
                    loop {
                        runner.run()?;
                        std::thread::sleep(Duration::from_millis(100));
                    }
                }() {
                    eprintln!("UI thread error: {}", e);
                }
            });
        }
    }

    loop {
        let ports = livi::EmptyPortConnections::new()
            .with_atom_sequence_inputs(std::iter::once(&input))
            .with_atom_sequence_outputs(std::iter::once(&mut output))
            .with_audio_outputs(outputs.iter_mut().map(|output| output.as_mut_slice()))
            .with_audio_inputs(inputs.iter_mut().map(|input| input.as_slice()));

        unsafe { instance.run(features.max_block_length(), ports).unwrap() };

        if let Some(inst) = ui_instance.as_ref() {
            for _msg in inst.pending_ui_control_msgs()? {
                println!("Discarding control message!");
            }
        }

        std::thread::sleep(Duration::from_millis(50));
    }
}

fn main() {
    if let Err(e) = main_impl() {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}
