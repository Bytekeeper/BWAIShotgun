cargo build --release
releng\7za a -pshotgun  -- bwaishotgun.7z target/release/bwaishotgun.exe bots tools tm SNP_DirectIP.snp game.toml shotgun.toml
releng\7za rn -pshotgun -- bwaishotgun.7z target/release/bwaishotgun.exe BWAIShotgun.exe
