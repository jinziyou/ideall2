use std::{fs, io::Cursor, path::PathBuf};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use clap::{Parser, Subcommand};
use ideall_acp::{ExternalAcpConfig, run_external_acp};
use ideall_agent::{CompletionProvider, ModelMessage, ModelRole, OpenAiCompatibleClient};
use ideall_application::LocalWorkspace;
use ideall_storage::{
    ArchiveLimits, Database, decrypt_workspace_archive, import_workspace_archive_atomic,
    is_encrypted_workspace_archive, parse_workspace_archive,
};
use ideall_sync::transport::HttpSyncTransport;
use ideall_updater::parse_verified_manifest;
use minisign::{SecretKeyBox, sign};

const PASSPHRASE_ENV: &str = "IDEALL_ARCHIVE_PASSPHRASE";
const MODEL_KEY_ENV: &str = "IDEALL_MODEL_API_KEY";
const SYNC_CODE_ENV: &str = "IDEALL_SYNC_CODE";
const SYNC_TOKEN_ENV: &str = "IDEALL_SYNC_BEARER_TOKEN";
const UPDATE_PRIVATE_KEY_ENV: &str = "IDEALL_UPDATE_PRIVATE_KEY";
const UPDATE_PRIVATE_KEY_PASSWORD_ENV: &str = "IDEALL_UPDATE_PRIVATE_KEY_PASSWORD";

#[derive(Parser)]
#[command(
    name = "ideall-cli",
    version,
    about = "ideall native maintenance tools"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Check schema, integrity, and authoritative row counts.
    Doctor {
        #[arg(long)]
        database: PathBuf,
    },
    /// Inspect or import a TypeScript V2 workspace archive.
    Archive {
        #[command(subcommand)]
        command: ArchiveCommand,
    },
    /// Run a one-shot ACP v1 handshake and prompt against an external stdio Agent.
    AcpProbe {
        #[arg(long)]
        program: PathBuf,
        #[arg(long = "arg")]
        args: Vec<String>,
        #[arg(long)]
        cwd: PathBuf,
        #[arg(long)]
        prompt: String,
    },
    /// Send one bounded request to a real OpenAI-compatible staging model.
    ModelProbe {
        #[arg(long)]
        base_url: String,
        #[arg(long)]
        model: String,
        #[arg(long)]
        prompt: String,
    },
    /// Round-trip a disposable note through all real synchronization domains.
    SyncProbe {
        #[arg(long)]
        database: PathBuf,
        #[arg(long)]
        server: String,
        #[arg(long)]
        seed_title: String,
    },
    /// Cryptographically verify a native desktop update manifest.
    UpdateVerify {
        #[arg(long)]
        manifest: PathBuf,
        #[arg(long)]
        signature: PathBuf,
        #[arg(long)]
        channel: String,
    },
    /// Sign an update or Registry payload with the protected Minisign root.
    UpdateSign {
        #[arg(long)]
        manifest: PathBuf,
        #[arg(long)]
        signature: PathBuf,
    },
}

#[derive(Subcommand)]
enum ArchiveCommand {
    /// Validate checksums, budgets, encryption, and payload structure without writing.
    Inspect { archive: PathBuf },
    /// Atomically import into a new SQLite database while retaining a backup.
    Import {
        archive: PathBuf,
        #[arg(long)]
        database: PathBuf,
    },
}

fn main() {
    if let Err(error) = run(Cli::parse()) {
        eprintln!("ideall-cli: {error}");
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<(), Box<dyn std::error::Error>> {
    match cli.command {
        Command::Doctor { database } => {
            let database = Database::open(database)?;
            database.quick_check()?;
            let counts = database.counts()?;
            println!("status: ok");
            println!("schema: {}", database.schema_version()?);
            println!("nodes: {}", counts.nodes);
            println!("blobs: {}", counts.blobs);
            println!("trashSnapshots: {}", counts.trash_snapshots);
            println!("plugins: {}", counts.plugins);
        }
        Command::Archive {
            command: ArchiveCommand::Inspect { archive },
        } => {
            let raw = fs::read_to_string(archive)?;
            let encrypted = is_encrypted_workspace_archive(&raw);
            let plaintext = archive_plaintext(&raw, encrypted)?;
            let archive = parse_workspace_archive(&plaintext, ArchiveLimits::default())?;
            println!("status: valid");
            println!("encrypted: {encrypted}");
            println!("exportedAt: {}", archive.exported_at);
            println!("checksum: {}", archive.manifest.checksum);
            println!("nodes: {}", archive.nodes.len());
            println!("blobs: {}", archive.blobs.len());
            println!("trashSnapshots: {}", archive.trash_snapshots.len());
            println!("plugins: {}", archive.plugins.len());
            println!(
                "tabs: {}",
                archive
                    .workspace
                    .as_ref()
                    .map_or(0, |state| state.tabs.len())
            );
        }
        Command::Archive {
            command: ArchiveCommand::Import { archive, database },
        } => {
            let raw = fs::read_to_string(archive)?;
            let passphrase = std::env::var(PASSPHRASE_ENV).ok();
            let result = import_workspace_archive_atomic(
                database,
                &raw,
                passphrase.as_deref(),
                ArchiveLimits::default(),
            )?;
            println!("status: imported");
            println!("database: {}", result.database_path.display());
            if let Some(backup) = result.backup_path {
                println!("backup: {}", backup.display());
            }
            println!("nodes: {}", result.counts.nodes);
            println!("blobs: {}", result.counts.blobs);
            println!("trashSnapshots: {}", result.counts.trash_snapshots);
            println!("plugins: {}", result.counts.plugins);
        }
        Command::AcpProbe {
            program,
            args,
            cwd,
            prompt,
        } => {
            let config = ExternalAcpConfig::new(program, args, cwd)?;
            let result = run_external_acp(config, &prompt)?;
            println!("status: {}", result.stop_reason);
            println!("tools: {}", result.tools.len());
            println!("deniedPermissions: {}", result.denied_permissions);
            println!("response: {}", result.content);
        }
        Command::ModelProbe {
            base_url,
            model,
            prompt,
        } => {
            let key = required_secret(MODEL_KEY_ENV)?;
            let mut client = OpenAiCompatibleClient::new(&base_url, &model, &key)?;
            let response = client.complete(&[ModelMessage::text(ModelRole::User, prompt)], &[])?;
            if !response.tool_calls.is_empty() {
                return Err("model returned tool calls when no tools were offered".into());
            }
            let content = response
                .content
                .as_deref()
                .filter(|content| !content.trim().is_empty())
                .ok_or("model returned an empty response")?;
            println!("status: ok");
            println!("responseChars: {}", content.chars().count());
        }
        Command::SyncProbe {
            database,
            server,
            seed_title,
        } => {
            if seed_title.trim().is_empty() || seed_title.chars().count() > 200 {
                return Err("sync seed title must contain 1 to 200 characters".into());
            }
            let code = required_secret(SYNC_CODE_ENV)?;
            let token = required_secret(SYNC_TOKEN_ENV)?;
            let mut workspace = LocalWorkspace::open(database)?;
            let note = workspace.create_note(None, seed_title.clone())?;
            let mut writer_transport = HttpSyncTransport::new(&server, &token)?;
            let first = workspace.sync_notes(&code, &mut writer_transport)?;

            let mut reader = LocalWorkspace::open_in_memory()?;
            let mut reader_transport = HttpSyncTransport::new(&server, &token)?;
            let readback = reader.sync_notes(&code, &mut reader_transport)?;
            if !reader
                .search(&seed_title, 10)?
                .iter()
                .any(|item| item.id == note.base().id)
            {
                return Err("synchronization read-back did not contain the seeded note".into());
            }

            workspace.move_to_trash(&note.base().id)?;
            let tombstone = workspace.sync_notes(&code, &mut writer_transport)?;
            let settled = reader.sync_notes(&code, &mut reader_transport)?;
            if reader
                .search(&seed_title, 10)?
                .iter()
                .any(|item| item.id == note.base().id)
            {
                return Err("synchronization tombstone did not settle on read-back".into());
            }

            let subscriptions = workspace.sync_subscriptions(&code, &mut writer_transport)?;
            let bookmarks = workspace.sync_bookmarks(&code, &mut writer_transport)?;
            println!("status: ok");
            println!(
                "notes: write={} read={} tombstone={} settled={}",
                first.total, readback.total, tombstone.total, settled.total
            );
            println!("subscriptions: {}", subscriptions.total);
            println!("bookmarks: {}", bookmarks.total);
        }
        Command::UpdateVerify {
            manifest,
            signature,
            channel,
        } => {
            let manifest = fs::read(manifest)?;
            let signature = fs::read(signature)?;
            let manifest = parse_verified_manifest(&manifest, &signature, &channel)?;
            println!("status: verified");
            println!("channel: {}", manifest.channel);
            println!("version: {}", manifest.version);
            println!("artifacts: {}", manifest.artifacts.len());
        }
        Command::UpdateSign {
            manifest,
            signature,
        } => {
            let private_key = required_secret(UPDATE_PRIVATE_KEY_ENV)?;
            let password = required_secret(UPDATE_PRIVATE_KEY_PASSWORD_ENV)?;
            let content = fs::read(&manifest)?;
            fs::write(
                signature,
                sign_update_payload(&private_key, &password, &content)?,
            )?;
            println!("status: signed");
            println!("bytes: {}", content.len());
        }
    }
    Ok(())
}

fn required_secret(name: &'static str) -> Result<String, Box<dyn std::error::Error>> {
    let value = std::env::var(name).map_err(|_| format!("{name} is required"))?;
    if value.is_empty() {
        return Err(format!("{name} is required").into());
    }
    Ok(value)
}

fn decode_private_key_document(value: &str) -> Result<String, Box<dyn std::error::Error>> {
    let value = value.trim();
    if value.starts_with("untrusted comment:") {
        return Ok(value.to_owned());
    }
    let decoded = STANDARD
        .decode(value)
        .map_err(|_| format!("{UPDATE_PRIVATE_KEY_ENV} is neither a Minisign key nor base64"))?;
    Ok(String::from_utf8(decoded)
        .map_err(|_| format!("{UPDATE_PRIVATE_KEY_ENV} is not valid UTF-8"))?)
}

fn sign_update_payload(
    private_key: &str,
    password: &str,
    content: &[u8],
) -> Result<String, Box<dyn std::error::Error>> {
    let key_document = decode_private_key_document(private_key)?;
    let secret_key =
        SecretKeyBox::from_string(&key_document)?.into_secret_key(Some(password.to_owned()))?;
    let signature_box = sign(
        None,
        &secret_key,
        Cursor::new(content),
        None,
        Some("signature from ideall native release key"),
    )?;
    // Preserve the historical updater `.sig` wire: base64 of the full
    // four-line Minisign signature document.
    Ok(STANDARD.encode(signature_box.into_string()))
}

fn archive_plaintext(raw: &str, encrypted: bool) -> Result<String, Box<dyn std::error::Error>> {
    if !encrypted {
        return Ok(raw.to_owned());
    }
    let passphrase = std::env::var(PASSPHRASE_ENV)
        .map_err(|_| format!("encrypted archive requires {PASSPHRASE_ENV}"))?;
    Ok(decrypt_workspace_archive(
        raw,
        &passphrase,
        ArchiveLimits::default(),
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use minisign::{KeyPair, SignatureBox, verify};

    #[test]
    fn native_update_signer_round_trips_the_historical_wire_format() {
        let password = "test-only-password";
        let payload = b"native update manifest";
        let key_pair =
            KeyPair::generate_encrypted_keypair(Some(password.to_owned())).expect("key pair");
        let private_key = key_pair
            .sk
            .to_box(None)
            .expect("secret key box")
            .to_string();
        let encoded_private_key = STANDARD.encode(private_key);

        let wire_signature =
            sign_update_payload(&encoded_private_key, password, payload).expect("signature");
        let signature_document = String::from_utf8(
            STANDARD
                .decode(wire_signature)
                .expect("base64 signature document"),
        )
        .expect("utf-8 signature document");
        let signature_box =
            SignatureBox::from_string(&signature_document).expect("Minisign signature");

        verify(
            &key_pair.pk,
            &signature_box,
            Cursor::new(payload),
            true,
            false,
            false,
        )
        .expect("valid signature");
    }
}
