cargo build --release
releng\7za a -pnone  -- bwaishotgun.7z target/release/bwaishotgun.exe bots SNP_DirectIP.snp bwheadless.exe game.toml shotgun.toml
releng\7za rn -pnone -- bwaishotgun.7z target/release/bwaishotgun.exe BWAIShotgun.exe