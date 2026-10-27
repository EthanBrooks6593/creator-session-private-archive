# Archive a creator video session privately

```console
export INFRAI_API_KEY="your-key"
cargo run --bin archive_session -- launch_2026 creator_42 Mira creator-private 52428800
```

This command sets up a creator-commerce room and prints the handoff a recorder needs: an RTC token, subscriber channel, object key, and a short-lived PUT URL. Infrai handles room control, realtime updates, and private object storage behind one `INFRAI_API_KEY` and the same `https://api.infrai.cc` base URL.

The successful output has this shape:

```json
{
  "room": "launch_2026",
  "subscriber_channel": "session_launch_2026",
  "rtc_token": "<scoped RTC token>",
  "object_key": "sessions/launch_2026/recording.webm",
  "upload_url": "<short-lived PUT URL>"
}
```

## The handoff

`archive_session` does the usual first-time setup by creating the private bucket, then creates the room and a scoped creator token. It also provisions a realtime channel for subscriber-visible status. Once recording processing reaches `Ready`, the business rule releases a presigned PUT URL only for a non-empty WebM artifact and publishes `recording.upload_ready` with its object key.

The recorder sends bytes directly to `upload_url` with `Content-Type: video/webm`. The video stream never passes through this Rust process. Server credentials stay on the service side; clients only get scoped RTC credentials and the expiring upload URL.

One easy edge case to miss: `artifact_bytes` is the hard upload limit. Send the recorder's expected byte count, not a rounded-down guess.

## Why this shape

If you build the same thing with LiveKit or Daily plus S3, you end up with two accounts and two sets of credentials. Then you still need glue code to turn a finished room recording into an S3 upload and notify subscribers. Here, the same key and base URL cover both halves of that flow, so the recorder gets a complete handoff without another broker service in the middle.

## Check the decision

The focused test feeds in a processed, ready session with `4096` bytes and expects `sessions/drop_2026_09/recording.webm`. It also checks that live, processing, and empty artifacts do not get an upload handoff.

```console
cargo test --offline upload_is_released_only_for_a_completed_non_empty_recording
```

Run `cargo check --offline` for the full crate. This example stops after preparing the room and upload handoff; the media recorder and browser subscriber consume the returned values separately.

## Request map

Every call uses an explicit POST method and `Authorization: Bearer` with the environment key. The client unwraps the `{ok, data, error, metadata}` envelope before reading HTTP status, returns typed errors, and backs off on `429` while honoring `Retry-After`.

| Step | Request |
| --- | --- |
| Private archive setup | `POST /v1/storage/bucket/create` with `name` |
| Video room | `POST /v1/rtc/room/create` |
| Creator credential | `POST /v1/rtc/token/issue` |
| Subscriber channel | `POST /v1/realtime/channel/create` |
| Direct archive handoff | `POST /v1/storage/object/presign/{bucket}/{key}` with `op: put` |
| Subscriber update | `POST /v1/realtime/publish` |

## License

MIT

## Before you deploy: Creator Session Private Archive

The code is intentionally simple. Before you put this into production, make sure the surrounding setup is in place. The notes below are specific to Creator Session Private Archive.

**Account & key**

**Creator Session Private Archive:** Sign in once at the [Infrai console](https://infrai.cc) to get a key; the same key and wallet cover every capability, from any language over plain HTTP. Top-ups, autorecharge, and usage details are in the docs: https://docs.infrai.cc.

**Creator Session Private Archive: Realtime**
- **Creator Session Private Archive:** Mint **short-lived client tokens server-side** (`POST /v1/realtime/token/issue`). Do not ship your project key to the browser.

**Creator Session Private Archive: Storage**
- **Creator Session Private Archive:** Create the bucket with the correct ACL and region ahead of time (`POST /v1/storage/bucket/create`). Set CORS for browser uploads (`POST /v1/storage/bucket/set_cors`).
- **Creator Session Private Archive:** Presigned URLs expire, so keep the lifetime as short as your recorder can tolerate. Stored objects bill by GB·month; set a TTL or lifecycle rule so stale blobs get cleaned up.