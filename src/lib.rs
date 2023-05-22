
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


    use warp::{hyper::{Response, Body}, Filter, filters::BoxedFilter};
    use warp_reverse_proxy::reverse_proxy_filter;

    use crate::{provider::Mapping};


    pub async fn start_server(port: u16, mappings: Vec<Mapping>, verbose: bool)
    {
        let log = warp::log::custom(move |info| {
            if verbose {
                println!(
                    "{} {} {}",
                    info.method(),
                    info.path(),
                    info.status(),
                );
            }
        });
        
        let host_mappings = mappings.iter().map(|m| m.host.clone()).collect::<Vec<String>>();
        let proxy_pac = format!(r###"
function FindProxyForURL (url, host) {{ 
let list = {};
for (let i = 0; i < list.length; i++) {{ 
if (host == list[i]) {{ 
    return 'PROXY localhost:7080';
}}
}}                  
return 'DIRECT';
}}"###, serde_json::to_string(&host_mappings).expect("fail"));


        let proxy_pac_file = Box::new(proxy_pac.into_boxed_str());
        
        let proxy_pac_response = |s_proxy_pac: String| {
            Response::builder()
                .header("my-custom-header", "some-value")
                .body(s_proxy_pac)
        };
        
        let proxy = warp::path!("proxy.pac")
            .map(move || { proxy_pac_response(proxy_pac_file.to_string()) });
            
        let mut filters: Vec<BoxedFilter<(Response<Body>,)>> = vec![];
        for mapping in mappings {
            let host_str = Box::leak(mapping.host.into_boxed_str());

            if verbose {
                println!("Mapping {} --> {}", host_str, mapping.target);
            }
            
            let filter = warp::any()
                .and(warp::header::exact("Host", host_str))
                .and(reverse_proxy_filter("".to_string(), mapping.target))
                .boxed();

            filters.push(filter);

            
        }

        for filter in filters {
            proxy = proxy.or(filter).boxed();
        }

        let proxy_with_log = proxy.with(log);
    
        warp::serve(proxy_with_log).run(([127, 0, 0, 1], port)).await;
    }


    pub fn create_filter(host: &str, target: String, verbose: bool) -> BoxedFilter<(Response<Body>,)> {

        if verbose {
            println!("Mapping {} --> {}", host, target);
        }
        
        return warp::any()
            .and(warp::header::exact("Host", host))
            .and(reverse_proxy_filter("".to_string(), target))
            .boxed();

        
    }
}
