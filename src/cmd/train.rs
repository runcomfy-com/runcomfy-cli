use anyhow::Result;

pub async fn run(base: String, dataset: String, use_case: String, gpu: String) -> Result<()> {
    // TODO:
    //   1. validate base ∈ supported set
    //   2. resolve use_case → AI Toolkit YAML template
    //   3. POST {trainer_api}/prod/v1/trainers/datasets to create dataset
    //   4. upload dataset files (multipart or signed URLs for large)
    //   5. wait for dataset READY
    //   6. POST {trainer_api}/prod/v1/trainers/ai-toolkit/jobs with config_file + gpu
    //   7. poll job status, stream progress
    //   8. on complete, download LoRA artifact
    let _ = (base, dataset, use_case, gpu);
    println!("train — not implemented yet");
    Ok(())
}
