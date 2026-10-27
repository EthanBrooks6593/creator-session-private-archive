use creator_session_archive::session_archive::{prepare_session, SessionRequest};
use creator_session_archive::infrai::InfraiClient;
use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("archive-session: {error}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let request = SessionRequest {
        session_id: argument(1, "session_id")?,
        creator_id: argument(2, "creator_id")?,
        creator_name: argument(3, "creator_name")?,
        bucket: argument(4, "bucket")?,
        artifact_bytes: argument(5, "artifact_bytes")?.parse()?,
    };
    let infrai = InfraiClient::from_env()?;
    let handoff = prepare_session(&infrai, &request).await?;
    println!("{}", serde_json::to_string_pretty(&handoff)?);
    Ok(())
}

fn argument(index: usize, name: &str) -> Result<String, Box<dyn std::error::Error>> {
    std::env::args()
        .nth(index)
        .ok_or_else(|| format!("missing {name}").into())
}

