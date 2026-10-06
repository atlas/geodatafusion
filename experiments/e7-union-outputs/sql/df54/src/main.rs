#[path = "../../matrix.rs"]
mod matrix;

#[tokio::main(flavor = "current_thread")]
async fn main() -> datafusion::error::Result<()> {
    matrix::run("DataFusion 54").await
}
