# HomeBack

The Backend of my Homeserver. Made to be used in combination with [HomeFront](https://github.com/tyssyt/HomeFront).
Expects the Environment Variables TWITCH_CLIENT_ID & TWITCH_CLIENT_SECRET to be set (see the [Twitch Authentication Guide](https://dev.twitch.tv/docs/authentication) for more Information).
To start a stream, [Streamlink](https://streamlink.github.io/) must be in the PATH and configured correctly.


## Build & Run

Run `cargo run` for a to build and run the backend. This runs the application under `127.0.0.1:23559`. You can override this by setting the Environment Variable ADDR.

Run `cargo build --target=aarch64-unknown-linux-gnu --release` to (cross-)compile an executable that can be run on a Raspberry Pi 4. An appropriate Toolchain must be installed. For Windows you can download one from [here](https://developer.arm.com/tools-and-software/open-source-software/developer-tools/gnu-toolchain/gnu-a/downloads) and set the environment Variables CC_aarch64_unknown_linux_gnu & AR_aarch64_unknown_linux_gnu to the executables in that toolchain.
