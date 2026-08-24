use crate::googleapis::google::ai::generativelanguage::v1beta::part::Data;
use crate::googleapis::google::ai::generativelanguage::v1beta::{Blob, Part};
use std::time::Duration;
use tokio::time::timeout;
use twilight_model::gateway::payload::incoming::MessageCreate;

const MAX_ATTACHMENT_BYTES: u64 = 20 * 1024 * 1024; // 20 MB
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(3);

pub async fn process_attachments(msg: &MessageCreate, user_prompt: &mut String) -> Vec<Part> {
    let mut parts = Vec::new();
    let reqwest_client = reqwest::Client::new();

    for attachment in msg.attachments.iter() {
        let mime_type = attachment.content_type.as_deref().unwrap_or("");

        tracing::debug!(
            filename = %attachment.filename,
            size_mb = attachment.size as f64 / 1024.0 / 1024.0,
            mime = ?attachment.content_type,
            "Processing attachment"
        );

        if attachment.size > MAX_ATTACHMENT_BYTES {
            tracing::warn!(filename = %attachment.filename, "Attachment exceeded size limit, skipping download");

            let status = format!(
                "\n[Attachment '{}' ({:.1}MB) was too large to process]",
                attachment.filename,
                attachment.size as f64 / 1024.0 / 1024.0
            );
            user_prompt.push_str(&status);
            continue;
        }

        if mime_type.starts_with("image/")
            || mime_type.starts_with("video/")
            || mime_type.starts_with("audio/")
        {
            let download = async {
                let resp = reqwest_client.get(&attachment.url).send().await?;
                let bytes = resp.bytes().await?;
                Ok::<_, anyhow::Error>((mime_type.to_string(), bytes))
            };

            match timeout(DOWNLOAD_TIMEOUT, download).await {
                Ok(Ok((mime, bytes))) => {
                    tracing::info!(filename = %attachment.filename, bytes = bytes.len(), "Attachment downloaded successfully");

                    parts.push(Part {
                        data: Some(Data::InlineData(Blob {
                            mime_type: mime,
                            data: bytes.to_vec(),
                        })),
                        ..Default::default()
                    });
                }
                _ => {
                    tracing::warn!(filename = %attachment.filename, "Attachment download timed out or failed");

                    user_prompt.push_str(&format!(
                        "\n[Attachment '{}' download timed out or failed]",
                        attachment.filename
                    ));
                }
            }
        }
    }

    parts
}
