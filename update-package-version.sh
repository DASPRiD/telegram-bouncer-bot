VERSION="$1"

cargo install cargo-edit
cargo set-version "$VERSION"
cargo update -w --offline
