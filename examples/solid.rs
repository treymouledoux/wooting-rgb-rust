use wooting_rgb_rust::{Frame, Rgb, Sdk};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut sdk = Sdk::new()?;
    let mut keyboard = sdk.device(0)?;
    let mut frame = Frame::new();
    frame.fill(Rgb::new(255, 80, 0));
    keyboard.set_frame(&frame)?;
    keyboard.flush()?;
    std::thread::sleep(std::time::Duration::from_secs(3));
    sdk.close()?;
    Ok(())
}
