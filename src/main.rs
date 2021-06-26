use clap::{App, Arg};
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Method, Request, Response, Server, StatusCode, Uri};
use log::{debug, error, info, warn};
use simplelog::{ColorChoice, ConfigBuilder, LevelFilter, TermLogger, TerminalMode};
use std::collections::{HashMap, VecDeque};
use std::convert::Infallible;
use std::fs;
use std::fs::read;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;

#[tokio::main]
async fn main() {
    configure_logging();
    let config = parse_config();
    info!("Starting httpserve on {}:{}", config.address, config.port);
    let addr = SocketAddr::from((config.address, config.port));

    let file_server = Arc::new(FileServer::new(PathBuf::from(config.dir)));

    let make_svc = make_service_fn(move |_conn| {
        let file_server = Arc::clone(&file_server);
        async move {
            Ok::<_, Infallible>(service_fn(move |req| {
                let file_server = Arc::clone(&file_server);
                async move { file_server.handle(req).await }
            }))
        }
    });

    let server = Server::bind(&addr)
        .serve(make_svc)
        .with_graceful_shutdown(async {
            tokio::signal::ctrl_c()
                .await
                .expect("failed to install CTRL+C signal handler")
        });

    if let Err(e) = server.await {
        error!("server error: {}", e);
    }
}

struct Config {
    dir: String,
    address: IpAddr,
    port: u16,
}

fn parse_config() -> Config {
    let matches = App::new("httpserve")
        .version("0.1")
        .author("James Guthrie")
        .about("Serve files from a directory")
        .arg(
            Arg::with_name("DIR")
                .value_name("DIR")
                .help("Set the directory to serve")
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
    Config { dir, address, port }
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
}

impl FileServer {
    pub fn new(dir: PathBuf) -> FileServer {
        let mut cache: HashMap<String, Vec<u8>> = HashMap::new();
        let mut to_visit: VecDeque<PathBuf> = VecDeque::from(vec![dir.clone()]);
        while to_visit.len() > 0 {
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
                            .strip_prefix((*&dir).to_str().expect("Path not Unicode"))
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
        return FileServer { cache };
    }

    async fn handle(&self, req: Request<Body>) -> Result<Response<Body>, Infallible> {
        let method = req.method();
        let uri = req.uri();
        info!("{} {}", method, uri);
        match *method {
            Method::GET => {
                let uri = if uri.eq("/") {
                    Uri::from_str("/index.html").unwrap()
                } else {
                    uri.clone()
                };
                let path = uri.to_string();
                let maybe_body = self.cache.get(&*path);
                maybe_body.map_or(
                    Ok::<_, Infallible>(
                        Response::builder()
                            .status(StatusCode::NOT_FOUND)
                            .body(Body::empty())
                            .expect("Unable to create `http::Response`"),
                    ),
                    |body: &Vec<u8>| {
                        Ok::<_, Infallible>(
                            Response::builder()
                                .status(StatusCode::OK)
                                .body(Body::from(body.to_owned()))
                                .expect("Unable to create `http::Response`"),
                        )
                    },
                )
            }
            _ => Ok::<_, Infallible>(
                Response::builder()
                    .status(StatusCode::METHOD_NOT_ALLOWED)
                    .body(Body::empty())
                    .expect("Unable to create `http::Response`"),
            ),
        }
    }
}
