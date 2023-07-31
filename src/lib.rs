mod tokiort;

pub mod config {
    use serde::{Deserialize, Serialize};

    use super::provider::Mapping;

    #[derive(Serialize, Deserialize, Debug, Default)]
    pub struct MyConfig {
        pub mappings: Vec<Mapping>,
    }
}

pub mod provider {

    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize, Debug, Clone)]
    pub struct Mapping {
        pub host: String,
        pub target: String,
    }

    impl Mapping {
        pub fn new(host: String, target: String) -> Self {
            Self { host, target }
        }
    }

    impl PartialEq for Mapping {
        fn eq(&self, other: &Self) -> bool {
            self.host == other.host
        }
    }
}

pub mod http {

    use std::net::IpAddr;
    use std::sync::Arc;
    use std::time::SystemTime;
    use std::{convert::Infallible, net::SocketAddr};

    use bytes::Bytes;
    use http_body_util::{combinators::BoxBody, BodyExt, Empty, Full};
    use hyper::body::Body;
    use hyper::client::conn::http1::Builder;
    use hyper::http::{HeaderValue, HeaderName};
    use hyper::server::conn::http1;
    use hyper::service::service_fn;
    use hyper::upgrade::Upgraded;
    use hyper::{Method, Request, Response, StatusCode, http, header};

    use tokio::net::{TcpListener, TcpStream};


    use crate::config::MyConfig;
    use crate::provider::Mapping;
    use crate::tokiort::tokiort::TokioIo;

    pub async fn start_server(port: u16, verbose: bool) -> Result<Response<BoxBody<Bytes, hyper::Error>>, hyper::Error> {
        let bind_addr = format!("127.0.0.1:{}", port);
        let addr: SocketAddr = bind_addr.parse().expect("Could not parse ip:port.");

        let listener = TcpListener::bind(addr).await.expect("fail");
        
        println!("Listening on http://{}", addr);

        loop {
            let (stream, _) = listener.accept().await.expect("fail");
            let io = TokioIo::new(stream);

            tokio::task::spawn(async move {
                if let Err(err) = http1::Builder::new()
                    .preserve_header_case(true)
                    .title_case_headers(true)
                    .serve_connection(io, service_fn(|req: Request<hyper::body::Incoming>| async move {
                        proxy(req, verbose).await
                    }))
                    .with_upgrades()
                    .await
                {
                    println!("Failed to serve connection: {:?}", err);
                }
            });
        }
    }

    fn create_pac_file(mappings: Vec<Mapping>) -> String {
        let host_mappings = mappings
            .iter()
            .map(|m| m.host.clone())
            .map(|m| {
                if m.ends_with(":443") {
                    return format!("https://{}", m.strip_suffix(":443").expect("fail")).to_string();
                } else if m.ends_with(":80") {
                    return format!("http://{}", m.strip_suffix(":80").expect("fail")).to_string();
                }
                return format!("https://{}", m).to_string();
            })
            .collect::<Vec<String>>();
        format!(
            r###"
function FindProxyForURL (url, host) {{ 
let list = {};
for (let i = 0; i < list.length; i++) {{ 
    if (host == list[i]) {{ 
        return 'PROXY localhost:7040';
    }}
}}                  
return 'DIRECT';
}}"###,
            serde_json::to_string(&host_mappings).expect("fail")
        )
    }

    async fn proxy(
        req: Request<hyper::body::Incoming>,
        verbose: bool
    ) -> Result<Response<BoxBody<Bytes, hyper::Error>>, hyper::Error> {

        if verbose { println!("Processing request of {}", req.uri().to_string()); }

        let config: MyConfig = confy::load("symfony-dev-proxy", None)
            .unwrap_or(MyConfig::default());

        if verbose { println!("Request method is {}", req.method().to_string()); }
        if Method::CONNECT == req.method() {
            for mapping in config.mappings {
                if let Some(addr) = host_addr(req.uri()) {
                    if (mapping.host.eq(&addr)) {
                        tokio::task::spawn(async move {
                            match hyper::upgrade::on(req).await {
                                Ok(upgraded) => {
                                    if let Err(e) = tunnel(upgraded, mapping.target).await {
                                        eprintln!("server io error: {}", e);
                                    };
                                }
                                Err(e) => eprintln!("upgrade error: {}", e),
                            }
                        });
        
                        return Ok(Response::new(empty()));
                    }
                } else {

                    eprintln!("CONNECT host is not socket addr: {:?}", req.uri());
                    let mut resp = Response::new(full("CONNECT must be to a socket address"));
                    *resp.status_mut() = http::StatusCode::BAD_REQUEST;

                    return Ok(resp);
                }
            }

            eprintln!("Could not find target for URI: {:?}", req.uri());
            let mut resp = Response::new(full("Could not find target for URI"));
            *resp.status_mut() = http::StatusCode::NOT_FOUND;

            return Ok(resp);
            
        } else {    
            
            if req.uri().path().starts_with("/proxy.pac") {
                let mut resp = Response::new(full(create_pac_file(config.mappings)));
                resp.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static("application/x-javascript-config"));

                return Ok(resp);
            }
        
            let response = Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(empty())
                .unwrap();
        
            Ok(response)
        }
    }

    fn host_addr(uri: &http::Uri) -> Option<String> {
        uri.authority().and_then(|auth| Some(auth.to_string()))
    }

    fn empty() -> BoxBody<Bytes, hyper::Error> {
        Empty::<Bytes>::new()
            .map_err(|never| match never {})
            .boxed()
    }

    fn full<T: Into<Bytes>>(chunk: T) -> BoxBody<Bytes, hyper::Error> {
        Full::new(chunk.into())
            .map_err(|never| match never {})
            .boxed()
    }

    // Create a TCP connection to host:port, build a tunnel between the connection and
    // the upgraded connection
    async fn tunnel(upgraded: Upgraded, addr: String) -> std::io::Result<()> {
        
        /*
        
        let root_cert_store = prepare_cert_store(&certificates);
    let tls_client_config = tls_config(certificates, root_cert_store);
    let tls_connector = TlsConnector::from(Arc::new(tls_client_config));

    let target = TcpStream::connect(target_address(&destination)).await?;

    let domain = rustls::ServerName::try_from(destination.0.as_str()).expect("Invalid DNSName");

    let mut tls_target = tls_connector.connect(domain, target).await?;
    debug!("TlS Connection ready");
         */
        
        // Connect to remote server
        let mut server = TcpStream::connect(addr).await?;
        let mut upgraded = TokioIo::new(upgraded);

        // Proxying data
        let (from_client, from_server) =
            tokio::io::copy_bidirectional(&mut upgraded, &mut server).await?;

        // Print message when done
        println!(
            "client wrote {} bytes and received {} bytes",
            from_client, from_server
        );

        Ok(())
    }
}

/*

async fn handle(client_ip: IpAddr, req: Request<Body>) -> Result<Response<Body>, Infallible> {
    println!("URI {}", req.uri());

    let config: MyConfig =
        confy::load("symfony-dev-proxy", None).unwrap_or(MyConfig::default());

    for mapping in &config.mappings {
        if let Some(host) = req.headers().get("host") {
            if host.eq(mapping.host.as_str()) {
                println!("* Proxy request");

                return match PROXY_CLIENT.call(client_ip, &mapping.target, req).await {
                    Ok(response) => Ok(response),
                    Err(_error) => Ok(Response::builder()
                        .status(StatusCode::INTERNAL_SERVER_ERROR)
                        .body(Body::from(format!("{:?}", _error)))
                        .unwrap()),
                };
            }
        }
    }
    if req.uri().path().starts_with("/proxy.pac") {
        return Ok(Response::new(Body::from(create_pac_file(config.mappings))));
    }

    let response = Response::builder()
        .status(StatusCode::NOT_FOUND)
        .body(Body::empty())
        .unwrap();

    Ok(response)
}
*/