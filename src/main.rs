use csv::WriterBuilder;
use local_ip_address::local_ip;
use mocopi_parser::SkeletonOrFrame;
use serde::Serialize;
use std::env;
use std::io::Cursor;
use std::net::UdpSocket;
use std::time::Instant;

#[derive(Serialize)]
struct Row<'a> {
    id: &'a str,
    time: &'a str,
    rot_x: f32,
    rot_y: f32,
    rot_z: f32,
    rot_w: f32,
    pos_x: f32,
    pos_y: f32,
    pos_z: f32,
}

fn main() -> () {
    let args: Vec<String> = env::args().collect();
    let local_ip = local_ip().unwrap();
    let port = match args.get(1) {
        Some(s) => s.clone(),
        None => String::from("12351"),
    };
    let addr = format!("{:?}:{}", local_ip, port);

    let socket = UdpSocket::bind(&addr).expect("couldn't bind socket");
    println!("Successfully {} binding socket", &addr);
    println!("Listening...");

    let mut buff = Cursor::new([0u8; 2048]);

    // csv
    let mut wtr = WriterBuilder::new()
        .has_headers(true)
        .from_path("output.csv")
        .unwrap();

    let start = Instant::now();
    loop {
        socket
            .recv_from(buff.get_mut())
            .expect("didn't receive data");

        match mocopi_parser::parse(buff.get_mut()) {
            Ok(r) => match r {
                SkeletonOrFrame::Skeleton(s) => {
                    dbg!(s);
                }
                SkeletonOrFrame::Frame(f) => {
                    dbg!(f.frame.bones.first());
                    let end = start.elapsed();
                    for b in f.frame.bones {
                        wtr.serialize(Row {
                            id: &b.id.to_string().as_str(),
                            time: format!("{}.{:03}", end.as_secs(), end.subsec_millis()).as_str(),
                            rot_x: b.trans.rot.x,
                            rot_y: b.trans.rot.y,
                            rot_z: b.trans.rot.z,
                            rot_w: b.trans.rot.w,
                            pos_x: b.trans.pos.x,
                            pos_y: b.trans.pos.y,
                            pos_z: b.trans.pos.z,
                        })
                        .unwrap();
                    }
                }
            },
            Err(_) => {
                dbg!("parse error");
            }
        }
    }
}
