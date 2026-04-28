use anyhow::Result;

pub async fn run(request_id: String, deployment_id: Option<String>) -> Result<()> {
    // TODO: GET /prod/v2/deployments/{deployment_id}/requests/{request_id}/status
    //       (deployment_id may be inferred from local request_id cache)
    let _ = (request_id, deployment_id);
    println!("status — not implemented yet");
    Ok(())
}
