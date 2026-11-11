use wooting_rgb_rust::Sdk;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut sdk = Sdk::new()?;
    let devices = sdk.devices()?;
    if devices.is_empty() {
        println!("No accessible Wooting keyboard found.");
    }
    for info in devices {
        println!("{info:#?}");
    }
    sdk.close()?;
    Ok(())
}
