cargo build --release
releng\7za a bwaishotgun.7z target/release/BWAIShotgun.exe bots SNP_DirectIP.snp bwheadless.exe game.toml shotgun.toml
releng\7za rn bwaishotgun.7z target/release/BWAIShotgun.exe BWAIShotgun.exe