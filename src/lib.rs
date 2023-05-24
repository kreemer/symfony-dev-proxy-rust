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
    use httproxide_hyper_reverse_proxy::ReverseProxy;
    use hyper::client::HttpConnector;
    use hyper::server::conn::AddrStream;
    use hyper::service::{make_service_fn, service_fn};
    use hyper::{Body, Request, Response, Server, StatusCode};
    use hyper_rustls::HttpsConnector;
    use rustls::client::ServerCertVerified;
    use rustls::{ClientConfig, RootCertStore};

    use std::net::IpAddr;
    use std::sync::Arc;
    use std::time::SystemTime;
    use std::{convert::Infallible, net::SocketAddr};

    use crate::config::MyConfig;
    use crate::provider::Mapping;

    lazy_static::lazy_static! {
        static ref PROXY_CLIENT: ReverseProxy<HttpsConnector<HttpConnector>, Body> = {

            let mut config = ClientConfig::builder()
                .with_safe_defaults()
                .with_root_certificates(RootCertStore::empty())
                .with_no_client_auth();

            config.dangerous()
                .set_certificate_verifier(Arc::new(NoCertificateVerification {}));

            let https = hyper_rustls::HttpsConnectorBuilder::new()
                .with_tls_config(config)
                .https_only()
                .enable_http1()
                .build();

            let hyper_client = hyper::Client::builder().build::<_, hyper::Body>(https);

            ReverseProxy::new(hyper_client)
        };
    }

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

    pub async fn start_server(port: u16, _verbose: bool) {
        let bind_addr = format!("127.0.0.1:{}", port);
        let addr: SocketAddr = bind_addr.parse().expect("Could not parse ip:port.");

        let make_svc = make_service_fn(move |conn: &AddrStream| {
            let remote_addr = conn.remote_addr().ip();
            async move { Ok::<_, Infallible>(service_fn(move |req| handle(remote_addr, req))) }
        });

        let server = Server::bind(&addr).serve(make_svc);

        println!("Running server on {:?}", addr);

        if let Err(e) = server.await {
            eprintln!("server error: {}", e);
        }
    }

    fn create_pac_file(mappings: Vec<Mapping>) -> String {
        let host_mappings = mappings
            .iter()
            .map(|m| m.host.clone())
            .collect::<Vec<String>>();
        format!(
            r###"
function FindProxyForURL (url, host) {{ 
let list = {};
for (let i = 0; i < list.length; i++) {{ 
if (host == list[i]) {{ 
    return 'PROXY localhost:7080';
}}
}}                  
return 'DIRECT';
}}"###,
            serde_json::to_string(&host_mappings).expect("fail")
        )
    }

    struct NoCertificateVerification {}
    impl rustls::client::ServerCertVerifier for NoCertificateVerification {
        fn verify_server_cert(
            &self,
            _end_entity: &rustls::Certificate,
            _intermediates: &[rustls::Certificate],
            _server_name: &rustls::ServerName,
            _scts: &mut dyn Iterator<Item = &[u8]>,
            _ocsp_response: &[u8],
            _now: SystemTime,
        ) -> Result<rustls::client::ServerCertVerified, rustls::Error> {
            Ok(ServerCertVerified::assertion())
        }
    }
}
