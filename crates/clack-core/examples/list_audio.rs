use cpal::traits::{DeviceTrait, HostTrait};

fn main() {
    let host = cpal::default_host();
    println!("Host: {:?}", host.id());

    let default = host.default_output_device();
    println!("Default output: {:?}", default.map(|d| d.name().unwrap_or("unknown".to_string())));

    println!("All output devices:");
    if let Ok(devices) = host.output_devices() {
        for (i, dev) in devices.enumerate() {
            println!("  {}: {}", i, dev.name().unwrap_or("unknown".to_string()));
        }
    }
}
