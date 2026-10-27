use crate::infrai::{InfraiClient, InfraiError};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use thiserror::Error;

#[derive(Debug, Clone)]
pub struct SessionRequest {
    pub session_id: String,
    pub creator_id: String,
    pub creator_name: String,
    pub bucket: String,
    pub artifact_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    Live,
    Processing,
    Ready,
}

#[derive(Debug, Error)]
pub enum ArchiveError {
    #[error(transparent)]
    Infrai(#[from] InfraiError),
    #[error("session and storage names may contain only ASCII letters, digits, '-' and '_'")]
    UnsafeName,
    #[error("an upload URL is issued only after an ended session has a non-empty artefact")]
    ArtifactNotReady,
}

#[derive(Debug, Serialize)]
pub struct SessionHandoff {
    pub room: String,
    pub subscriber_channel: String,
    pub rtc_token: String,
    pub object_key: String,
    pub upload_url: String,
}

#[derive(Debug, Deserialize)]
struct TokenData {
    token: String,
}

#[derive(Debug, Deserialize)]
struct PresignData {
    url: String,
}

#[derive(Serialize)]
struct BucketCreate<'a> {
    name: &'a str,
}

#[derive(Serialize)]
struct RoomCreate<'a> {
    name: &'a str,
    max_participants: u16,
    empty_timeout_s: u32,
    region: &'a str,
}

#[derive(Serialize)]
struct RtcTokenIssue<'a> {
    room: &'a str,
    identity: &'a str,
    display_name: &'a str,
    ttl_s: u32,
    can_publish: bool,
    can_subscribe: bool,
}

#[derive(Serialize)]
struct ChannelCreate<'a> {
    channel: &'a str,
    r#type: &'a str,
    vendor: &'a str,
}

#[derive(Serialize)]
struct Publish<'a> {
    channel: &'a str,
    event: &'a str,
    data: Value,
    account_id: &'a str,
}

#[derive(Serialize)]
struct PutPresign<'a> {
    op: &'a str,
    expires_seconds: u32,
    content_type: &'a str,
    max_bytes: u64,
    idempotency_key: &'a str,
}

pub fn artifact_key(request: &SessionRequest, state: SessionState) -> Result<String, ArchiveError> {
    if state != SessionState::Ready || request.artifact_bytes == 0 {
        return Err(ArchiveError::ArtifactNotReady);
    }
    Ok(format!("sessions/{}/recording.webm", request.session_id))
}

pub async fn prepare_session(
    infrai: &InfraiClient,
    request: &SessionRequest,
) -> Result<SessionHandoff, ArchiveError> {
    for value in [&request.session_id, &request.creator_id, &request.bucket] {
        if !is_safe_segment(value) {
            return Err(ArchiveError::UnsafeName);
        }
    }

    // infrai.storage.bucket.create
    let _: Value = infrai
        .post("/v1/storage/bucket/create", &BucketCreate { name: &request.bucket })
        .await?;

    // infrai.rtc.room.create
    let _: Value = infrai
        .post(
            "/v1/rtc/room/create",
            &RoomCreate {
                name: &request.session_id,
                max_participants: 24,
                empty_timeout_s: 300,
                region: "auto",
            },
        )
        .await?;

    // infrai.rtc.token.issue
    let token: TokenData = infrai
        .post(
            "/v1/rtc/token/issue",
            &RtcTokenIssue {
                room: &request.session_id,
                identity: &request.creator_id,
                display_name: &request.creator_name,
                ttl_s: 3600,
                can_publish: true,
                can_subscribe: true,
            },
        )
        .await?;

    let subscriber_channel = format!("session_{}", request.session_id);
    // infrai.realtime.channel.create
    let _: Value = infrai
        .post(
            "/v1/realtime/channel/create",
            &ChannelCreate {
                channel: &subscriber_channel,
                r#type: "broadcast",
                vendor: "infrai",
            },
        )
        .await?;

    let object_key = artifact_key(request, SessionState::Ready)?;
    let presign_path = format!(
        "/v1/storage/object/presign/{}/{}",
        request.bucket, object_key
    );
    let idempotency_key = format!("{}-recording-put", request.session_id);
    // infrai.storage.object.presign
    let presign: PresignData = infrai
        .post(
            &presign_path,
            &PutPresign {
                op: "put",
                expires_seconds: 900,
                content_type: "video/webm",
                max_bytes: request.artifact_bytes,
                idempotency_key: &idempotency_key,
            },
        )
        .await?;

    // infrai.realtime.publish
    let _: Value = infrai
        .post(
            "/v1/realtime/publish",
            &Publish {
                channel: &subscriber_channel,
                event: "recording.upload_ready",
                data: json!({
                    "session_id": request.session_id,
                    "object_key": object_key,
                }),
                account_id: &request.creator_id,
            },
        )
        .await?;

    Ok(SessionHandoff {
        room: request.session_id.clone(),
        subscriber_channel,
        rtc_token: token.token,
        object_key,
        upload_url: presign.url,
    })
}

fn is_safe_segment(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(bytes: u64) -> SessionRequest {
        SessionRequest {
            session_id: "drop_2026_09".to_owned(),
            creator_id: "creator_42".to_owned(),
            creator_name: "Mira".to_owned(),
            bucket: "creator-private".to_owned(),
            artifact_bytes: bytes,
        }
    }

    #[test]
    fn upload_is_released_only_for_a_completed_non_empty_recording() {
        assert!(matches!(
            artifact_key(&request(4_096), SessionState::Live),
            Err(ArchiveError::ArtifactNotReady)
        ));
        assert!(matches!(
            artifact_key(&request(4_096), SessionState::Processing),
            Err(ArchiveError::ArtifactNotReady)
        ));
        assert!(matches!(
            artifact_key(&request(0), SessionState::Ready),
            Err(ArchiveError::ArtifactNotReady)
        ));
        assert_eq!(
            artifact_key(&request(4_096), SessionState::Ready).unwrap(),
            "sessions/drop_2026_09/recording.webm"
        );
    }
}
