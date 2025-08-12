
cd backend
cargo build --release

cd ..
cp backend/target/release/uds_http_request /usr/lib/koreader/plugins/raku*/
cp backend/target/release/server /usr/lib/koreader/plugins/raku*/
