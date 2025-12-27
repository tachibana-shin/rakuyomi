
cd backend
cargo build --release

cd ..
cp backend/target/release/uds_http_request ~/.config/koreader/plugins/raku*/
cp backend/target/release/server ~/.config/koreader/plugins/raku*/
cp backend/target/release/cbz_metadata_reader ~/.config/koreader/plugins/raku*/
