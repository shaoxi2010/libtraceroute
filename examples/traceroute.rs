extern crate libtraceroute;

use libtraceroute::{Traceroute, Config};
use libtraceroute::util::{Protocol, get_available_interfaces};
use clap::{Command, arg, value_parser};


fn main() {
	   let matches = Command::new("Traceroute")
        .version("0.1.1")
        .author("Rafael R. G. <rafael@rafaelgaia.com>")
        .about("Traceroute implementation in Rust")
        .arg(arg!(-i --interface <INTERFACE> "Interface to use").value_parser(value_parser!(String)).required(true))
        .arg(arg!(-p --protocol <PROTOCOL> "Protocol to use (ICMP, UDP or TCP)").value_parser(value_parser!(String)).required(true))
		.arg(arg!(-t --timeout [TIMEOUT] "Timeout in milliseconds").value_parser(value_parser!(u64).range(1..)))
        .arg(arg!(-m --maxhops [MAX_HOPS] "Maximum number of hops").value_parser(value_parser!(u32).range(1..255)))
		.arg(arg!(-f --frist_ttl [TTL] "First TTL").value_parser(value_parser!(u8).range(1..255)))
		.arg(arg!(--mtu [MTU] "Test Max MTU").value_parser(value_parser!(u16).range(80..1500)))
		.arg(arg!(--queries [QUERIES] "Max queries").value_parser(value_parser!(u32).range(1..)))
		.arg(arg!(<DESTINATION_IP> "Destination IP address").value_parser(value_parser!(String)))
		.arg(arg!(<DESTINATION_MAC> "Destination MAC address").value_parser(value_parser!(String)))
		.get_matches();

	   let available_interfaces = get_available_interfaces();

    let network_interface = match available_interfaces.iter().filter(|i| i.name == matches.get_one::<String>("interface").unwrap().as_str()).next() {
        Some(i) => i.clone(),
        None => panic!("no such interface available")
    };

	let protocol = match matches.get_one::<String>("protocol").unwrap().as_str() {
		"ICMP" => Protocol::ICMP,
		"UDP" => Protocol::UDP,
		"TCP" => Protocol::TCP,
		_ => panic!("no such protocol available")
	};
    let mut traceroute_query = Traceroute::new(matches.get_one::<String>("DESTINATION_IP").unwrap(),
	matches.get_one::<String>("DESTINATION_MAC").unwrap(),
	 Config::default()
        .with_port(33480)
        .with_max_hops(*matches.get_one::<u32>("maxhops").unwrap_or(&20))
        .with_first_ttl(*matches.get_one::<u8>("frist_ttl").unwrap_or(&1))
        .with_interface(network_interface)
		.with_max_mtu(*matches.get_one::<u16>("mtu").unwrap_or(&80))
        .with_number_of_queries(*matches.get_one::<u32>("queries").unwrap_or(&2))
        .with_protocol(protocol)
        .with_timeout(*matches.get_one::<u64>("timeout").unwrap_or(&1000)));

    // Calculate all hops upfront
    let traceroute_result = traceroute_query.perform_traceroute();

    // Iterate over pre-calculated hops vector
    for hop in traceroute_result {
        print!("{}", hop.ttl);
        for query_result in &hop.query_result {
            print!(" \t{}ms \t{}\n", query_result.rtt.as_millis(), query_result.addr);
        }
    }
}