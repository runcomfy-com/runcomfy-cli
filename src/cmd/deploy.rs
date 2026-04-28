use anyhow::Result;

use crate::cli::DeployCommand;

pub async fn dispatch(cmd: DeployCommand) -> Result<()> {
    match cmd {
        DeployCommand::Create { skill } => create(skill).await,
        DeployCommand::List => list().await,
        DeployCommand::Delete { deployment_id } => delete(deployment_id).await,
    }
}

/// Look up `skill` in the official workflow registry, then call
/// Serverless API `create_deployment(workflow_id)`. Cache the resulting
/// deployment_id locally so subsequent `runcomfy run <skill>` calls reuse it.
async fn create(skill: String) -> Result<()> {
    // TODO:
    //   1. registry::lookup(skill) → workflow_id (or UnknownSkill error)
    //   2. POST /prod/v2/deployments with { workflow_id, autoscale: { min:0 } }
    //   3. config::save_deployment_mapping(skill, deployment_id)
    //   4. print deployment_id + status URL
    let _ = skill;
    println!("deploy create — not implemented yet");
    Ok(())
}

async fn list() -> Result<()> {
    // TODO: GET /prod/v2/deployments → table view
    println!("deploy list — not implemented yet");
    Ok(())
}

async fn delete(deployment_id: String) -> Result<()> {
    // TODO: DELETE /prod/v2/deployments/{deployment_id}
    let _ = deployment_id;
    println!("deploy delete — not implemented yet");
    Ok(())
}
