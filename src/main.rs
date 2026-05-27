use clap::{crate_version, App, Arg};
use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, Empty, Full};
use hyper::body::{Bytes, Incoming};
use hyper::header::LOCATION;
use hyper::http::uri::Builder as UriBuilder;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto::Builder;
use hyper_util::server::graceful::GracefulShutdown;
use log::{debug, error, info, warn};
use simplelog::{ColorChoice, ConfigBuilder, LevelFilter, TermLogger, TerminalMode};
use std::collections::{HashMap, VecDeque};
use std::convert::Infallible;
use std::fs;
use std::fs::read;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::pin::pin;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() {
    configure_logging();
    let config = parse_config();
    info!("Starting httpserve on {}:{}", config.address, config.port);
    let addr = SocketAddr::from((config.address, config.port));

    let file_server = Arc::new(FileServer::new(
        PathBuf::from(config.dir),
        config.redirect_http,
    ));

    let listener = TcpListener::bind(&addr)
        .await
        .unwrap_or_else(|_| panic!("unable to bind {}:{}", addr.ip(), addr.port()));

    let server = Builder::new(TokioExecutor::new());

    let graceful = GracefulShutdown::new();

    let mut ctrl_c = pin!(tokio::signal::ctrl_c());

    loop {
        tokio::select! {
            Ok((stream, peer_addr)) = listener.accept() => {
                let stream = TokioIo::new(Box::pin(stream));
                let file_server = Arc::clone(&file_server);
                let conn = server.serve_connection_with_upgrades(stream, service_fn(move |req| {
                   let file_server = Arc::clone(&file_server);
                    async move { file_server.handle(req).await }
                }));

                let conn = graceful.watch(conn.into_owned());

                tokio::spawn(async move {
                    if let Err(err) = conn.await {
                        error!("connection error: {}", err);
                    }
                    warn!("connection dropped: {}", peer_addr);
                });
            },
            _ = ctrl_c.as_mut() => {
                drop(listener);
                info!("graceful shutdown request received");
                break;
            }
        }
    }

    tokio::select! {
        _ = graceful.shutdown() => {
            info!("server shutdown gracefully");
        },
        _ = tokio::time::sleep(Duration::from_secs(10)) => {
            warn!("timed out waiting for connections to close");
        }
    }
}

struct Config {
    dir: String,
    address: IpAddr,
    port: u16,
    redirect_http: bool,
}

fn parse_config() -> Config {
    let matches = App::new("httpserve")
        .version(crate_version!())
        .author("James Guthrie")
        .about("Serve files from a directory")
        .arg(
            Arg::with_name("DIR")
                .value_name("DIR")
                .help("Set the directory to serve")
                .required(true)
                .takes_value(true),
        )
        .arg(
            Arg::with_name("port")
                .short("p")
                .long("port")
                .value_name("PORT")
                .help("Set the port to listen on")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("address")
                .short("a")
                .long("address")
                .value_name("ADDRESS")
                .help("Sets the address to bind to")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("redirect")
                .short("r")
                .long("redirect-http")
                .help("Whether to redirect http to https"),
        )
        .get_matches();

    let dir = matches.value_of("DIR").unwrap().to_string();
    let address = matches
        .value_of("address")
        .map_or(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), |addr| {
            addr.parse::<IpAddr>().expect("Unable to parse IP address")
        });
    let port = matches.value_of("port").map_or(3000, |p| {
        p.parse::<u16>().expect("Unable to parse port number")
    });
    let redirect_http = matches.is_present("redirect");
    Config {
        dir,
        address,
        port,
        redirect_http,
    }
}

fn configure_logging() {
    let config = ConfigBuilder::new()
        .set_target_level(LevelFilter::Trace)
        .set_time_format(String::from("%Y-%m-%dT%H:%M:%S%.3f"))
        .build();
    let _ = TermLogger::init(
        LevelFilter::Debug,
        config,
        TerminalMode::Stdout,
        ColorChoice::Auto,
    );
}

struct FileServer {
    cache: HashMap<String, Vec<u8>>,
    http_to_https_redirect: bool,
}

impl FileServer {
    pub fn new(dir: PathBuf, http_to_https_redirect: bool) -> FileServer {
        let mut cache: HashMap<String, Vec<u8>> = HashMap::new();
        let mut to_visit: VecDeque<PathBuf> = VecDeque::from(vec![dir.clone()]);
        while !to_visit.is_empty() {
            match to_visit.pop_front() {
                Some(item) => {
                    if item.is_dir() {
                        let children = fs::read_dir(&item).expect("Failed to read directory");
                        children.into_iter().for_each(|child| {
                            let new_path = child.expect("Unable to traverse directory").path();
                            to_visit.push_back(new_path);
                        });
                    } else {
                        let copy = item.to_owned();
                        let file_path = copy.to_str().expect("Path not Unicode");
                        let path = file_path
                            .strip_prefix(dir.to_str().expect("Path not Unicode"))
                            .unwrap();
                        let content = read(item).expect("Failed to read file");
                        debug!("Loaded {} bytes from {}", content.len(), path);
                        cache.insert(path.to_owned(), content);
                    }
                }
                None => {
                    warn!("Queue was empty. This was not expected.");
                }
            }
        }
        FileServer {
            cache,
            http_to_https_redirect,
        }
    }

    async fn handle(
        &self,
        req: Request<Incoming>,
    ) -> Result<Response<BoxBody<Bytes, Infallible>>, Infallible> {
        let method = req.method();
        let uri = req.uri();

        info!("{} {}", method, uri);

        let response = match *method {
            Method::GET => {
                self.build_https_redirect(&req).unwrap_or_else(|| {
                    let mut path = uri.path().to_string();
                    if !self.cache.contains_key(&*path) {
                        // apply a simple fallback rule to fetch index.html
                        if uri.path().ends_with("/") {
                            path = uri.path().to_string() + "index.html";
                        }
                    }
                    let maybe_body = self.cache.get(&*path);
                    match maybe_body {
                        Some(body) => Response::builder()
                            .status(StatusCode::OK)
                            .body(Full::new(Bytes::from(body.to_owned())).boxed())
                            .expect("Unable to create `http::Response`"),
                        None => Response::builder()
                            .status(StatusCode::NOT_FOUND)
                            .body(Empty::new().boxed())
                            .expect("Unable to create `http::Response`"),
                    }
                })
            }
            _ => Response::builder()
                .status(StatusCode::METHOD_NOT_ALLOWED)
                .body(Empty::new().boxed())
                .expect("Unable to create `http::Response`"),
        };
        Ok(response)
    }

    /// A simple http -> https redirect, based on the presence of the `x-forwarded-proto` header in
    /// the request. This is as described in the following fly.io blog post:
    /// https://fly.io/blog/always-be-connecting-with-https/
    fn build_https_redirect(
        &self,
        req: &Request<Incoming>,
    ) -> Option<Response<BoxBody<Bytes, Infallible>>> {
        let uri = req.uri();
        if !self.http_to_https_redirect {
            return None;
        }

        let fwd_proto = req.headers().get("x-forwarded-proto");
        fwd_proto?;

        if fwd_proto.unwrap() != "http" {
            return None;
        }

        let path_and_query = uri.path_and_query().expect("No path and query");

        // Determining the current host can go via two methods:
        // - in http1.1 and earlier: via the "host" header set on the request
        // - in http2 onwards: via the "authority" component of the Uri
        let host = req.headers().get("host").map_or_else(
            // Unwrap here and on the line below should only cause a problem if the authority ot
            // hostname do not contain ASCII characters. Ignore this edge case for now.
            || uri.authority().unwrap().as_str(),
            |v| v.to_str().unwrap(),
        );

        let https_request = UriBuilder::new()
            .scheme("https")
            .path_and_query(path_and_query.clone())
            .authority(host)
            .build()
            .unwrap();

        info!("Redirecting to https for {}", path_and_query);

        Some(
            Response::builder()
                .status(StatusCode::MOVED_PERMANENTLY)
                .header(LOCATION, https_request.to_string())
                .body(Empty::new().boxed())
                .expect("Unable to create https redirect"),
        )
    }
}
