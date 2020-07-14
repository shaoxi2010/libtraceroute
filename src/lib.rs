/*
Copyright (c) 2020 Ilya Grishkov

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
 */

extern crate pnet;

use pnet::packet::ethernet::{EtherTypes, EthernetPacket};
use pnet::packet::icmp::{IcmpPacket, IcmpTypes};
use pnet::packet::ip::IpNextHeaderProtocols;
use pnet::packet::ipv4::Ipv4Packet;
use pnet::packet::Packet;
use pnet::datalink::{channel, NetworkInterface, DataLinkReceiver};
use std::net::IpAddr;
use std::net::UdpSocket;
use std::time::Duration;

pub struct TracerouteQuery {
    addr: String,
    port: u16,
    max_hops: u32,
    interface: NetworkInterface,
    ttl: u32,
    udp_socket: UdpSocket,
    datalink_receiver: Box<dyn DataLinkReceiver>,
    done: bool,
}

pub struct TracerouteHop {
    pub ttl: u32,
    pub rtt: Duration,
    pub addr: String,
}

impl Iterator for TracerouteQuery {
    type Item = TracerouteHop;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }

        let hop = self.get_next_hop();
        match hop {
            Ok(h) => {
                self.done = h.addr == self.addr;
                Some(h)
            }
            Err(_) => None
        }
    }
}

impl TracerouteQuery {
    /// Creates new instance of TracerouteQuery.
    pub fn new(addr: String, port: u16, max_hops: u32) -> Self {
        let socket = UdpSocket::bind("0.0.0.0:33434")
            .expect("couldn't bind socket");

        let default_interface = get_default_interface()
            .expect("couldn't find default interface");

        let (_, rx) = match channel(&default_interface, Default::default()) {
            Ok(pnet::datalink::Channel::Ethernet(tx, rx)) => (tx, rx),
            Ok(_) => panic!("libtraceroute: unhandled channel type"),
            Err(e) => panic!("libtraceroute: unable to create channel: {}", e),
        };

        TracerouteQuery {
            addr,
            port,
            max_hops,
            interface: default_interface,
            ttl: 1,
            udp_socket: socket,
            datalink_receiver: rx,
            done: false,
        }
    }

    // TODO: find a cleaner and more reliable way of timeout.
    /// Get next hop on the route. Increases TTL.
    pub fn get_next_hop(&mut self) -> Result<TracerouteHop, &'static str> {
        let now = std::time::SystemTime::now();
        self.udp_socket.set_ttl(self.ttl).expect("couldn't set TTL");

        self.udp_socket.send_to("".as_ref(), (self.addr.as_str(), self.port)).expect("couldn't send to UDP packet");

        let hop_ip: String = loop {
            match process_incoming_packet(&mut self.datalink_receiver, &self.interface) {
                Ok(ip) => break ip,
                Err(_) => {
                    match now.elapsed() {
                        Ok(t) => {
                            if t.as_secs() > 2 {
                                break String::from("*");
                            }
                        }
                        Err(_) => {}
                    }
                }
            }
        };
        self.ttl += 1;
        Ok(TracerouteHop { ttl: self.ttl - 1, rtt: Duration::from_millis(10), addr: hop_ip })
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
}

// TODO: add checks for ICMP code icmp[1] = 0 and icmp[1] = 3
/// Processes ICMP packets. Returns addresses of packets that conform to the following
/// Berkeley Packet filter formula: `icmp and (icmp[0] = 11) or (icmp[0] = 3)`, thus
/// accepting all ICMP packets that have information about the status of UDP packets used for
/// traceroute.
fn handle_icmp_packet(source: IpAddr, packet: &[u8]) -> Result<String, &'static str> {
    let icmp_packet = IcmpPacket::new(packet).expect("malformed ICMP packet");

    match icmp_packet.get_icmp_type() {
        IcmpTypes::TimeExceeded => Ok(source.to_string()),
        IcmpTypes::DestinationUnreachable => Ok(source.to_string()),
        _ => Err("wrong packet")
    }
}

/// Processes IPv4 packet and passes it on to transport layer packet handler.
fn handle_ipv4_packet(packet: &[u8]) -> Result<String, &'static str> {
    let header = Ipv4Packet::new(packet).expect("malformed IPv4 packet");

    let source = IpAddr::V4(header.get_source());
    let payload = header.payload();

    match header.get_next_level_protocol() {
        IpNextHeaderProtocols::Icmp => handle_icmp_packet(source, payload),
        _ => Err("wrong packet")
    }
}

/// Processes ethernet frame and rejects all packets that are not IPv4.
fn handle_ethernet_frame(packet: &[u8]) -> Result<String, &'static str> {
    let ethernet = EthernetPacket::new(packet).expect("malformed Ethernet frame");
    match ethernet.get_ethertype() {
        EtherTypes::Ipv4 => return handle_ipv4_packet(ethernet.payload()),
        _ => Err("wrong packet")
    }
}

/// Start capturing packets until until expected ICMP packet was received.
fn process_incoming_packet(rx: &mut Box<dyn DataLinkReceiver>,
                           interface: &NetworkInterface) -> Result<String, &'static str> {
    match rx.next() {
        Ok(packet) => {
            if cfg!(any(target_os = "macos", target_os = "ios"))
                && interface.is_up() && !interface.is_broadcast()
                && ((!interface.is_loopback() && interface.is_point_to_point())
                || interface.is_loopback()) {
                let payload_offset = if interface.is_loopback() { 14 } else { 0 };
                if packet.len() > payload_offset {
                    return handle_ipv4_packet(&packet[payload_offset..]);
                }
            }
            return handle_ethernet_frame(packet);
        }
        Err(e) => panic!("libtraceroute: unable to receive packet: {}", e),
    }
}

// TODO: find a more reliable way of detecting the default interface
// TODO: accept users specified interface
// NOTE: current implementation doesn't work on Windows
/// Returns the first interface that is up, loopback and has an IP address associated with it.
fn get_default_interface() -> Result<NetworkInterface, &'static str> {
    let interfaces = pnet::datalink::interfaces();
    let interface = interfaces
        .iter()
        .filter(|e| e.is_up() && !e.is_loopback() && e.ips.len() > 0)
        .next();

    match interface {
        Some(i) => Ok(i.clone()),
        None => Err("libtraceroute: couldn't find default interface")
    }
}
