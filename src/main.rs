use anyhow::Result;
use clap::Parser;
use hyper::{Body, Method, Request, Response, StatusCode};
use log::info;
use std::future::Future;
use std::path::{Component, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

#[derive(Debug, Parser)]
#[clap(about, version)]
struct Args {
    /// Bind to this address:port.
    #[clap(short, long, value_name = "ADDRESS:PORT", default_value = "[::1]:0")]
    bind: std::net::SocketAddr,

    /// Set the path of the document root.
    #[clap(short = 'r', long, value_name = "PATH", default_value = ".")]
    document_root: PathBuf,
}

#[tokio::main]
async fn main() -> Result<()> {
    simplelog::SimpleLogger::init(log::LevelFilter::Info, simplelog::Config::default())?;

    let args = Args::parse();

    let server = hyper::Server::try_bind(&args.bind)?.serve(Server::new(args));
    info!("Server started on {}", server.local_addr());

    server.await?;

    Ok(())
}

struct Server {
    args: Arc<Args>,
}

impl Server {
    fn new(args: Args) -> Self {
        Self {
            args: Arc::new(args),
        }
    }
}

impl<T> hyper::service::Service<T> for Server {
    type Response = Service;
    type Error = hyper::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, _: T) -> Self::Future {
        let svc = Service::new(Arc::clone(&self.args));
        Box::pin(async move { Ok(svc) })
    }
}

struct Service {
    args: Arc<Args>,
}

impl Service {
    fn new(args: Arc<Args>) -> Self {
        Self { args }
    }

    fn resolve(&self, uri: &str) -> PathBuf {
        let uri = PathBuf::from(uri);
        let mut normalized = PathBuf::new();
        for component in uri.components() {
            match component {
                Component::ParentDir => {
                    normalized.pop();
                }
                Component::Normal(component) => normalized.push(component),
                _ => {}
            }
        }

        self.args.document_root.join(normalized)
    }
}

impl hyper::service::Service<Request<Body>> for Service {
    type Response = Response<Body>;
    type Error = hyper::http::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        use tokio::fs::File;
        use tokio_util::codec::{BytesCodec, FramedRead};

        let path = self.resolve(req.uri().path());
        let fut = async move {
            let res = if req.method() != Method::GET {
                Err(StatusCode::METHOD_NOT_ALLOWED)
            } else if path.is_dir() {
                Err(StatusCode::FORBIDDEN)
            } else if let Ok(file) = File::open(path).await {
                Ok(Body::wrap_stream(FramedRead::new(file, BytesCodec::new())))
            } else {
                Err(StatusCode::NOT_FOUND)
            };

            match res {
                Ok(body) => Response::builder().body(body),
                Err(status) => Response::builder().status(status).body(Body::empty()),
            }
        };

        Box::pin(fut)
    }
}
