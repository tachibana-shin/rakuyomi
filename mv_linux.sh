
cd backend
cargo build

cd ..
cp backend/target/debug/uds_http_request ~/.config/koreader/plugins/raku*/
cp backend/target/debug/server ~/.config/koreader/plugins/raku*/
cp backend/target/debug/cbz_metadata_reader ~/.config/koreader/plugins/raku*/
