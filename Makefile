bwaishotgun_linux: game_table_exe
	cargo build --release --target=x86_64-unknown-linux-gnu

game_table_exe:
	cargo build --release -p game_table --target=i686-pc-windows-gnu
