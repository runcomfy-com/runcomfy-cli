use anyhow::Result;

/// Run a model_id (Model API) or a registered skill name (auto-deploys workflow).
///
/// `target` heuristic:
/// - contains '/' → treat as Model API model_id
/// - otherwise → treat as registered skill name (look up in workflow registry,
///   create deployment if missing, run via Serverless API)
pub async fn run(
    target: String,
    input: Option<String>,
    input_file: Option<String>,
    _wait: bool,
    _poll_secs: u64,
) -> Result<()> {
    // TODO:
    //   1. resolve target → (api_kind, target_id)
    //      - if target.contains('/') → ModelApi(model_id)
    //      - else → look up registry; if not found, error::UnknownSkill
    //              if no deployment cached, auto-deploy via cmd::deploy
    //   2. parse input JSON (from --input or --input-file)
    //   3. submit_request → request_id
    //   4. if wait { poll get_request_status until terminal, then get_request_result }
    //   5. print result URLs
    let _ = (target, input, input_file);
    println!("run — not implemented yet");
    Ok(())
}
