
#[macro_use]
extern crate tracing;

pub mod config {
    use serde::{Serialize, Deserialize};

    use super::provider::Mapping;

    #[derive(Serialize, Deserialize, Debug)]
    pub struct MyConfig {
        pub mappings: Vec<Mapping>
    }

    impl ::std::default::Default for MyConfig {
        fn default() -> Self { Self { mappings: vec![] } }
        
    }
}

pub mod provider {

    use serde::{Serialize, Deserialize};

    #[derive(Serialize, Deserialize, Debug, Clone)]
    pub struct Mapping {
        pub host: String,
        pub target: String,
    }

    impl Mapping {
        pub fn new(host: String, target: String) -> Self { Self { host, target } }
    }

    impl PartialEq for Mapping {
        fn eq(&self, other: &Self) -> bool {
            self.host == other.host
        }
    }
}

pub mod http {
    use httproxide_hyper_reverse_proxy::ReverseProxy;
    use hyper::body::HttpBody;
    use hyper::client::HttpConnector;
    use hyper::server::conn::AddrStream;
    use hyper::service::{make_service_fn, service_fn};
    use hyper::{Body, Request, Response, Server, StatusCode, client, Client};
    use hyper_rustls::{HttpsConnectorBuilder, HttpsConnector};
    use hyper_trust_dns::{TrustDnsResolver, RustlsHttpsConnector};
    use native_tls::{TlsConnectorBuilder, TlsConnector};
    use rustls::client::ServerCertVerified;
    use rustls::{ConfigBuilder, ClientConfig, RootCertStore};
    
    use std::net::IpAddr;
    use std::sync::Arc;
    use std::time::SystemTime;
    use std::{convert::Infallible, net::SocketAddr};
    
    use openssl::ssl::{SslConnector, SslMethod, SslVerifyMode};


    
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
            
            return ReverseProxy::new(hyper_client);
        };
    }

    fn debug_request(req: Request<Body>) -> Result<Response<Body>, Infallible>  {
        let body_str = format!("{:?}", req);
        Ok(Response::new(Body::from(body_str)))
    }
    
    
    async fn handle(client_ip: IpAddr, req: Request<Body>) -> Result<Response<Body>, Infallible> {
        println!("URI {}", req.uri().to_string());

        let config: MyConfig = confy::load("symfony-dev-proxy", None).unwrap_or(MyConfig::default());

        for mapping in &config.mappings {
            if let Some(host) = req.headers().get("host") {
                if host.eq(mapping.host.as_str()) {
                    println!("* Proxy request");

                    return match PROXY_CLIENT.call(client_ip, &mapping.target, req).await {
                        Ok(response) => {Ok(response)}
                        Err(_error) => {Ok(Response::builder()
                                            .status(StatusCode::INTERNAL_SERVER_ERROR)
                                            .body(Body::from(format!("{:?}", _error)))
                                            .unwrap())}
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

        return Ok(response)

    }

    pub async fn start_server(port: u16, verbose: bool) 
    {
        let bind_addr = format!("127.0.0.1:{}", port);
        let addr: SocketAddr = bind_addr.parse().expect("Could not parse ip:port.");

        let make_svc = make_service_fn(move |conn: &AddrStream| {
            ;
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
        let host_mappings = mappings.iter().map(|m| m.host.clone()).collect::<Vec<String>>();
        return format!(r###"
function FindProxyForURL (url, host) {{ 
let list = {};
for (let i = 0; i < list.length; i++) {{ 
if (host == list[i]) {{ 
    return 'PROXY localhost:7080';
}}
}}                  
return 'DIRECT';
}}"###, serde_json::to_string(&host_mappings).expect("fail"));

    }


    struct NoCertificateVerification {}
    impl rustls::client::ServerCertVerifier for NoCertificateVerification {
        fn verify_server_cert(
            &self,
            end_entity: &rustls::Certificate,
            intermediates: &[rustls::Certificate],
            server_name: &rustls::ServerName,
            scts: &mut dyn Iterator<Item = &[u8]>,
            ocsp_response: &[u8],
            now: SystemTime
        ) -> Result<rustls::client::ServerCertVerified, rustls::Error>
        {
            return Ok(ServerCertVerified::assertion());
        }
    }

}
