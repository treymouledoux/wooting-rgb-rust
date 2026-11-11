# wooting-rgb-rust

Rust 1.88+ wrapper for the official [Wooting RGB SDK](https://github.com/WootingKb/wooting-rgb-sdk). The native SDK lives in `vendor/wooting-rgb-sdk` as a pinned Git submodule, including its HIDAPI dependency. Cargo builds both from source and generates bindings from the pinned headers.

The safe interface provides device discovery, owned metadata, device selection, single-key colors, full frames, buffered updates, reset, and feature responses. The `sys` module exposes the upstream C API for advanced callers. Firmware protocol handling comes from the official SDK, including its multi-report implementation.

## Build

```sh
git clone --recurse-submodules https://github.com/treymouledoux/wooting-rgb-rust.git
cd wooting-rgb-rust
cargo test
cargo run --example devices
```

An existing checkout can initialize dependencies with `git submodule update --init --recursive`.

| Platform | Build requirements |
| --- | --- |
| Windows | Rust MSVC toolchain, Visual Studio C++ Build Tools and Windows SDK, LLVM/libclang |
| macOS | Rust and Xcode Command Line Tools with libclang. The native build uses Clang for Apple's framework headers. |
| Linux | Rust, C compiler, libclang, pkg-config and libudev development headers. On Ubuntu install `build-essential libclang-dev pkg-config libudev-dev`. |

Set `LIBCLANG_PATH` to the directory containing libclang if it cannot be discovered. Linux also needs device permissions through an appropriate Wooting udev rule. The bundled HIDAPI uses the Linux hidraw backend. Supported build targets are Windows, macOS and Linux.

## Use

```toml
[dependencies]
wooting-rgb-rust = { git = "https://github.com/treymouledoux/wooting-rgb-rust" }
```

Cargo fetches the nested submodules for Git dependencies. For local development beside this repository, use `wooting-rgb-rust = { path = "../wooting-rgb-rust" }`. Applications can pin a reviewed wrapper commit with `rev`.

```rust
use wooting_rgb_rust::{Frame, Rgb, Sdk};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut sdk = Sdk::new()?;
    for info in sdk.devices()? {
        println!("{}: {} ({} rows × {} columns)",
            info.index, info.model, info.rows, info.columns);
    }

    let mut keyboard = sdk.device(0)?;
    let mut frame = Frame::new();
    frame.set(1, 2, Rgb::new(255, 80, 0))?;
    keyboard.set_frame(&frame)?;
    keyboard.flush()?;
    std::thread::sleep(std::time::Duration::from_secs(3));
    sdk.close()?;
    Ok(())
}
```

`Frame` always contains 378 RGB bytes with a 21-column row stride. Use `DeviceInfo` for each keyboard's matrix dimensions. Some matrix positions have no physical LED. The 60% models retain an empty first row in the SDK matrix.

Keep one `Sdk` for the lifetime of the application. It owns the native global state and rejects a second live instance. Use `Mutex<Sdk>` to share it across threads. A `Device` exclusively borrows the SDK, protecting selection across a complete frame. `Drop` restores keyboard lighting and closes handles. Explicit `close()` reports reset failures. Calling `std::process::exit` or forcibly killing the process bypasses destructors.

`Sdk::new()` succeeds when no keyboard is connected. `device(0)` then returns `DeviceUnavailable`. After a disconnect, finish the device borrow and call `refresh()` to enumerate again. Device indexes can change after refresh. Hardware writes return errors if the SDK reports failure, and failed frames can be retried in full after reconnecting.

`feature_response()` allocates the protocol's required buffer and returns only the received bytes. Command IDs and payload layouts are firmware-specific. Raw `sys` callers must uphold the initialization, bounds, pointer and synchronization requirements documented on that module, including avoiding concurrent calls with the safe API.

## Update the official SDK

```sh
git -C vendor/wooting-rgb-sdk fetch origin
git -C vendor/wooting-rgb-sdk checkout <reviewed-upstream-commit>
git submodule update --init --recursive
cargo test
cargo clippy --all-targets -- -D warnings
git add vendor/wooting-rgb-sdk
git commit -m "Update official Wooting RGB SDK"
```

The initial pin is `7e091a0`, which includes the multi-report feature-response fix merged in September 2026 and support for 80HE and 60HE V2. Updating the pin regenerates bindings from those exact headers on the next build.

## License

The wrapper and official Wooting SDK use MPL-2.0. The upstream license remains in the submodule. HIDAPI's license options and notices are preserved in its nested submodule.
