use anyhow::{Result, Context, bail};
use std::path::PathBuf;
use std::fs;
use clap::{Args, ValueEnum};
use pkcs8::EncodePublicKey;
use rsa::traits::PublicKeyParts;

#[derive(Args)]
#[command[name = "keygen", about = "Simple key generator for Ed25519, RSA and P-256"]]
pub struct KeygenArgs {
    output: String,
    #[arg(short = 'a', long, value_enum, default_value_t = Algorithm::Ed25519)]
    algorithm: Algorithm,
    #[arg(long, default_value_t = 3072)]
    bits: usize,
    #[arg(short = 'p', long)]
    pem_public: bool,
}

#[derive(Clone, Copy, ValueEnum, Debug)]
pub enum Algorithm {
    Ed25519,
    Rsa,
    P256,
}

pub fn generate_key(a: KeygenArgs) -> Result<()> {
    match a.algorithm {
        Algorithm::Ed25519 => generate_ed25519(&a),
        Algorithm::Rsa => generate_rsa(&a),
        Algorithm::P256 => generate_p256(&a),
    }
}

fn generate_ed25519(a: &KeygenArgs) -> Result<()> {
    use ed25519_dalek::{pkcs8::EncodePrivateKey as _, SigningKey, VerifyingKey};
    use ssh_key::public::{PublicKey as SshPublicKey, Ed25519PublicKey as SshEd25519Pub};
    use rand_core_old::OsRng;

    // Generate
    let signing_key = SigningKey::generate(&mut OsRng);
    let verifying_key: VerifyingKey = signing_key.verifying_key();

    // Private PEM
    let pem_private = signing_key.to_pkcs8_pem(pkcs8::LineEnding::LF)?.to_string();
    let private_path = PathBuf::from(format!("{}.pem", a.output));
    write(&private_path, pem_private.as_bytes())?;

    // Public SSH
    let ssh_ed25519 = SshEd25519Pub::from(&verifying_key);
    let ssh_public = SshPublicKey::from(ssh_ed25519);
    let public_line = ssh_public.to_openssh()?.to_string() + "\n";
    let public_path = PathBuf::from(format!("{}.pub", a.output));
    write(&public_path, public_line.as_bytes())?;

    // Public PEM
    if a.pem_public {
        let der_public = verifying_key.to_public_key_der()?;
        let pem_public = der_public.to_pem("PUBLIC KEY", ssh_key::LineEnding::LF)?;
        let public_pem_path = PathBuf::from(format!("{}.pub.pem", a.output));
        write(&public_pem_path, pem_public.as_bytes())?;
    }
    Ok(())
}

fn generate_rsa(a: &KeygenArgs) -> Result<()> {
    use rsa::{pkcs8::EncodePrivateKey as _, pkcs8::EncodePublicKey as _, RsaPrivateKey, RsaPublicKey};
    use ssh_key::{public::{PublicKey as SshPublicKey, RsaPublicKey as SshRsaPub}};
    use rand_core_old::OsRng;

    if a.bits < 2048 {
        bail!("RSA bits should be >= 2048")
    }

    // Generate
    let signing_key = RsaPrivateKey::new(&mut OsRng, a.bits).with_context(|| "RSA key generation failed")?;
    let public_key = RsaPublicKey::from(&signing_key);

    // Private PEM
    let pem_private = signing_key.to_pkcs8_pem(pkcs8::LineEnding::LF)?.to_string();
    let private_path = PathBuf::from(format!("{}.pem", a.output));
    write(&private_path, pem_private.as_bytes())?;

    // Public SSH
    let n_rsa = public_key.n().to_bytes_be();
    let e_rsa = public_key.e().to_bytes_be();
    let ssh_rsa = SshRsaPub {
        e: ssh_key::Mpint::from_bytes(&e_rsa)?,
        n: ssh_key::Mpint::from_bytes(&n_rsa)?,
    };
    let ssh_public = SshPublicKey::from(ssh_rsa);
    let public_line = ssh_public.to_openssh()?.to_string() + "\n";
    let public_path = PathBuf::from(format!("{}.pub", a.output));
    write(&public_path, public_line.as_bytes())?;

    // Public PEM
    if a.pem_public {
        let pem_public = public_key.to_public_key_pem(pkcs8::LineEnding::LF)?.to_string();
        let public_pem_path = PathBuf::from(format!("{}.pub.pem", a.output));
        write(&public_pem_path, pem_public.as_bytes())?;
    }
    Ok(())
}

fn generate_p256(a: &KeygenArgs) -> Result<()> {
    use p256::{ecdsa::{SigningKey, VerifyingKey}, pkcs8::EncodePrivateKey as _, PublicKey as P256PublicKey};
    use ssh_key::public::{EcdsaPublicKey, PublicKey as SshPublicKey};
    use rand_core_old::OsRng;

    // Generate
    let signing_key = SigningKey::random(&mut OsRng);
    let verifying_key = VerifyingKey::from(&signing_key);
    let public_key: P256PublicKey = verifying_key.into();

    // Private PEM
    let pem_private = signing_key.to_pkcs8_pem(pkcs8::LineEnding::LF)?;
    let private_path = PathBuf::from(format!("{}.pem", a.output));
    write(&private_path, pem_private.as_bytes())?;

    // Public SSH
    let ssh_ecdsa = EcdsaPublicKey::from(&verifying_key);
    let ssh_public = SshPublicKey::from(ssh_ecdsa);
    let public_line = ssh_public.to_openssh()?.to_string() + "\n";
    let public_path = PathBuf::from(format!("{}.pub", a.output));
    write(&public_path, public_line.as_bytes())?;

    // Public PEM
    if a.pem_public {
        let pem_public = public_key.to_public_key_pem(pkcs8::LineEnding::LF)?;
        let public_pem_path = PathBuf::from(format!("{}.pub.pem", a.output));
        write(&public_pem_path, pem_public.as_bytes())?;
    }
    Ok(())
}

fn write(path: &PathBuf, data: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    fs::write(path, data).with_context(|| format!("writing {}", path.display()))
}

