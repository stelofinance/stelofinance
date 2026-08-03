mod app;
mod auth;

#[tokio::main]
async fn main() {
	topcoat::start(app::router().await).await.unwrap();
}
