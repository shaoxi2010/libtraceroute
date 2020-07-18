/*
   Copyright 2020 Ilya Grishkov

   Licensed under the Apache License, Version 2.0 (the "License");
   you may not use this file except in compliance with the License.
   You may obtain a copy of the License at

       http://www.apache.org/licenses/LICENSE-2.0

   Unless required by applicable law or agreed to in writing, software
   distributed under the License is distributed on an "AS IS" BASIS,
   WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
   See the License for the specific language governing permissions and
   limitations under the License.
*/

extern crate pnet;

pub mod util;

use std::net::Ipv4Addr;
use std::time::Duration;
use std::str::FromStr;

pub struct Traceroute {
    addr: String,
    port: u16,
    max_hops: u32,
    number_of_queries: u32,
    ttl: u8,
    done: bool,
    channel: util::Channel,
}

pub struct TracerouteHop {
    pub ttl: u8,
    pub query_result: Vec<TracerouteQueryResult>,
}

pub struct TracerouteQueryResult {
    pub rtt: Duration,
    pub addr: String,
}

impl Iterator for Traceroute {
    type Item = TracerouteHop;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }

        let hop = self.calculate_next_hop();
        match hop {
            Ok(h) => {
                self.done = h.query_result.iter()
                    .filter(|ip| ip.addr == self.addr)
                    .next().is_some()
                    || self.ttl > self.max_hops as u8;
                Some(h)
            }
            Err(_) => None
        }
    }
}

impl Traceroute {
    /// Creates new instance of TracerouteQuery.
    pub fn new(addr: &str) -> Self {
        Traceroute {
            addr: String::from(addr),
            port: 33434,
            max_hops: 30,
            number_of_queries: 3,
            ttl: 1,
            done: false,
            channel: Default::default(),
        }
    }

    /// Builder: Port for traceroute. Will be incremented on every query.
    pub fn with_port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    /// Builder: Maximum number of hops.
    pub fn with_max_hops(mut self, max_hops: u32) -> Self {
        self.max_hops = max_hops;
        self
    }

    /// Builder: Number of queries to run per hop.
    pub fn with_number_of_queries(mut self, number_of_queries: u32) -> Self {
        self.number_of_queries = number_of_queries;
        self
    }

    /// Builder: Interface to send packets from.
    pub fn with_interface(mut self, interface: &str) -> Self {
        let available_interfaces = util::get_available_interfaces()
            .expect("couldn't get available interfaces");

        let default_interface = match available_interfaces.iter().filter(|i| i.name == interface).next() {
            Some(i) => i.clone(),
            None => panic!("no such interface available")
        };

        self.channel = util::Channel::new(&default_interface);
        self
    }

    /// Builder: First TTL to record.
    pub fn with_first_ttl(mut self, first_ttl: u8) -> Self {
        self.ttl = first_ttl;
        self
    }

    /// Returns a vector of traceroute hops.
    pub fn perform_traceroute(&mut self) -> Vec<TracerouteHop> {
        let mut hops = Vec::<TracerouteHop>::new();
        for _ in 1..self.max_hops {
            if self.done {
                return hops;
            }
            match self.next() {
                Some(hop) => hops.push(hop),
                None => {}
            }
        }
        return hops;
    }

    /// Get next hop on the route. Increases TTL.
    fn calculate_next_hop(&mut self) -> Result<TracerouteHop, &'static str> {
        let mut query_results = Vec::<TracerouteQueryResult>::new();
        for _ in 0..self.number_of_queries {
            match self.get_next_query_result() {
                Ok(v) => {
                    if query_results.iter()
                        .filter(|query_result| query_result.addr == v.addr)
                        .next().is_none() {
                        query_results.push(v)
                    }
                }
                Err(_) => query_results.push(TracerouteQueryResult { rtt: Duration::from_millis(0), addr: String::from("*") })
            }
        }
        self.ttl += 1;
        Ok(TracerouteHop { ttl: self.ttl - 1, query_result: query_results })
    }
    
    /// Runs a query to the destination and returns RTT and IP of the router where
    /// time-to-live-exceeded. Doesn't increase TTL.
    fn get_next_query_result(&mut self) -> Result<TracerouteQueryResult, &'static str> {
        let now = std::time::SystemTime::now();

        let buf = self.channel.build_udp_packet(Ipv4Addr::from_str(self.addr.as_str()).expect("malformed destination ip"), self.ttl, self.port);

        self.channel.send(&buf);
        let hop_ip = self.channel.recv();
        Ok(TracerouteQueryResult {
            rtt: now.elapsed().unwrap_or(Duration::from_millis(0)),
            addr: hop_ip,
        })
    }
}
