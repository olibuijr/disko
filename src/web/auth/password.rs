use anyhow::{Context, Result, bail};
use argon2::{Argon2, PasswordHash, PasswordHasher, password_hash::SaltString};
use std::{
    fs::OpenOptions,
    io::{Read, Write},
    os::unix::fs::OpenOptionsExt,
    path::Path,
};

pub fn random_token() -> Result<String> {
    let mut bytes = [0_u8; 32];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

pub fn set_password(path: &Path) -> Result<()> {
    let mut input = String::new();
    std::io::stdin().take(1025).read_to_string(&mut input)?;
    let password = input.trim_end_matches(['\r', '\n']);
    if password.len() < 4 || password.len() > 1024 {
        bail!("password must be 4–1024 bytes, supplied through stdin");
    }
    let salt = SaltString::encode_b64(random_token()?.as_bytes().get(..32).unwrap())
        .map_err(|e| anyhow::anyhow!("salt: {e}"))?;
    let hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| anyhow::anyhow!("password hash: {e}"))?
        .to_string();
    let parent = path.parent().context("password file needs a parent")?;
    std::fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(".password-{}", random_token()?));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temporary)?;
    file.write_all(hash.as_bytes())?;
    file.sync_all()?;
    if let Err(e) = std::fs::rename(&temporary, path) {
        let _ = std::fs::remove_file(&temporary);
        return Err(e.into());
    }
    eprintln!(
        "Password updated in {}; restart Disko to apply it",
        path.display()
    );
    Ok(())
}

pub fn load(path: &Path) -> Result<String> {
    let hash = std::fs::read_to_string(path).with_context(|| {
        format!(
            "read dashboard password {}; create it with `disko password`",
            path.display()
        )
    })?;
    PasswordHash::new(hash.trim()).map_err(|e| anyhow::anyhow!("invalid password hash: {e}"))?;
    Ok(hash.trim().to_owned())
}
