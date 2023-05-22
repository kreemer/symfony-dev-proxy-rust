
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

    #[derive(Serialize, Deserialize, Debug)]
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


    use std::task::{Poll, Context};

    use axum::{
        body::Body,

        extract::State,
        http::uri::Uri,
        response::{IntoResponse, Response},
        routing::get,
        Router,
    };
    use axum::routing::MethodRouter;
    use hyper::Request;
    use crate::{provider::Mapping, config::{MyConfig}};
    use tower::{ServiceBuilder, Layer, Service};
    

    use hyper::client::HttpConnector;

    type Client = hyper::client::Client<HttpConnector, Body>;


    use futures_util::future::BoxFuture;


    pub async fn start_server(port: u16, mappings: Vec<Mapping>, verbose: bool)
    {

        let app = Router::new()
            .merge(proxy_pac_router())
            .layer(ServiceBuilder::new().layer(ProxyLayer).layer(LoggingLayer));

        // run it with hyper on localhost:3000
        axum::Server::bind(&format!("0.0.0.0:{}", port).parse().unwrap())
            .serve(app.into_make_service())
            .await
            .unwrap();
    }


    fn proxy_pac_router() -> Router {
        async fn handler() -> &'static str {
            Box::leak(create_pac_file(vec![]).into_boxed_str())
        }
    
        route("/proxy.pac", get(handler))
    }

    fn route(path: &str, method_router: MethodRouter<()>) -> Router {
        Router::new().route(path, method_router)
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


    #[derive(Clone)]
    struct LoggingLayer;

    impl<S> Layer<S> for LoggingLayer {
        type Service = LoggingMiddleware<S>;

        fn layer(&self, inner: S) -> Self::Service {
            LoggingMiddleware { inner }
        }
    }

    #[derive(Clone)]
    struct LoggingMiddleware<S> {
        inner: S,
    }

    impl<S> Service<Request<Body>> for LoggingMiddleware<S>
    where
        S: Service<Request<Body>, Response = axum::response::Response> + Send + 'static,
        S::Future: Send + 'static,
    {
        type Response = S::Response;
        type Error = S::Error;
        // `BoxFuture` is a type alias for `Pin<Box<dyn Future + Send + 'a>>`
        type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

        fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            self.inner.poll_ready(cx)
        }

        fn call(&mut self, request: Request<Body>) -> Self::Future {
            println!("{}", "request");
            let future = self.inner.call(request);
            Box::pin(async move {
                let response: axum::response::Response = future.await?;
                Ok(response)
            })
        }
    }


    #[derive(Clone)]
    struct ProxyLayer;

    impl<S> Layer<S> for ProxyLayer {
        type Service = ProxyMiddleware<S>;

        fn layer(&self, inner: S) -> Self::Service {
            ProxyMiddleware { inner }
        }
    }

    #[derive(Clone)]
    struct ProxyMiddleware<S> {
        inner: S,
    }

    impl<S> Service<Request<Body>> for ProxyMiddleware<S>
    where
        S: Service<Request<Body>, Response = axum::response::Response> + Send + 'static,
        S::Future: Send + 'static,
    {
        type Response = S::Response;
        type Error = S::Error;
        // `BoxFuture` is a type alias for `Pin<Box<dyn Future + Send + 'a>>`
        type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

        fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            self.inner.poll_ready(cx)
        }

        fn call(&mut self, mut request: Request<Body>) -> Self::Future {
            let config: MyConfig = confy::load("symfony-dev-proxy", None).unwrap_or(MyConfig::default());
        
            let headers = request.headers();
            if let Some(host) = headers.get("Host") {
                let host_value = &host.to_str().unwrap_or("").to_string();
                let host_mapping = config.mappings.iter().find(|m| &m.host == host_value);

                if let Some(mapping) = host_mapping {
                    println!("Request {} to {}", mapping.host, mapping.target);
                    
                    let client: Client = hyper::Client::builder().build(HttpConnector::new());

                    let path = request.uri().path();
                    let path_query = request
                        .uri()
                        .path_and_query()
                        .map(|v| v.as_str())
                        .unwrap_or(path);

                    let uri = format!("{}{}", mapping.target, path_query);

                    *request.uri_mut() = Uri::try_from(uri).unwrap();


                    return Box::pin(async move {
                        let proxy_response = client.request(request).await;

                        let response: axum::response::Response = proxy_response.unwrap().into_response();
                        Ok(response)
                    });                    
                }
            }

            let future = self.inner.call(request);
            Box::pin(async move {
                let response: axum::response::Response = future.await?;
                Ok(response)
            })
        }
    }


}
