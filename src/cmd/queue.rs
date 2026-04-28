use anyhow::Result;

use crate::cli::QueueCommand;

pub async fn dispatch(cmd: QueueCommand) -> Result<()> {
    match cmd {
        QueueCommand::List => list().await,
        QueueCommand::Cancel {
            request_id,
            deployment_id,
        } => cancel(request_id, deployment_id).await,
    }
}

async fn list() -> Result<()> {
    // TODO: list active/queued requests across user's deployments
    println!("queue list — not implemented yet");
    Ok(())
}

async fn cancel(request_id: String, deployment_id: Option<String>) -> Result<()> {
    // TODO: POST /prod/v2/deployments/{deployment_id}/requests/{request_id}/cancel
    let _ = (request_id, deployment_id);
    println!("queue cancel — not implemented yet");
    Ok(())
}
