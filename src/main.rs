use clap::Parser;
use hyper::http::{Error as HttpError, Method, Request, Response, StatusCode};
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Error, Server};
use log::info;
use std::path::PathBuf;
use std::sync::Arc;

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
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    use simplelog::{Config, SimpleLogger};

    SimpleLogger::init(log::LevelFilter::Info, Config::default())
        .expect("failed to initialize logging");

    let args = Arc::new(Args::parse());

    let service = make_service_fn(|_| {
        let args = Arc::clone(&args);
        async { Ok::<_, Error>(service_fn(move |req| handle(Arc::clone(&args), req))) }
    });

    let server = Server::try_bind(&args.bind)?.serve(service);
    info!("Server started on {}", server.local_addr());

    tokio::select! {
        r = server => r?,
        r = handle_signal() => r?,
    }

    Ok(())
}

#[cfg(unix)]
async fn handle_signal() -> Result<(), std::io::Error> {
    use tokio::signal::unix::{signal, SignalKind};

    let mut interrupt = signal(SignalKind::interrupt())?;
    let mut terminate = signal(SignalKind::terminate())?;

    tokio::select! {
        _ = interrupt.recv() => (),
        _ = terminate.recv() => (),
    };

    Ok(())
}

#[cfg(not(unix))]
async fn handle_signal() -> Result<(), std::io::Error> {
    Ok(tokio::signal::ctrl_c().await?)
}

async fn handle(args: Arc<Args>, req: Request<Body>) -> Result<Response<Body>, HttpError> {
    use tokio::fs::File;
    use tokio_util::codec::{BytesCodec, FramedRead};

    let mut path = args.document_root.join(normalize(req.uri().path()));
    if path.is_dir() {
        path.push("index.html");
    }

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
}

fn normalize(uri: &str) -> PathBuf {
    use std::path::Component;

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

    normalized
}
