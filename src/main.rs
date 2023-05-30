#[macro_use]
extern crate lazy_static;

use std::cell::{Ref, RefCell};
use std::collections::HashMap;
use csv::WriterBuilder;
use local_ip_address::local_ip;
use mocopi_parser::SkeletonOrFrame;
use serde::Serialize;
use std::{env, thread};
use std::error::Error;
use std::fs::File;
use std::io::{Cursor, Read, stdin, Write};
use std::net::UdpSocket;
use std::path::Path;
use std::process::Command;
use std::sync::{Arc, mpsc, Mutex, MutexGuard};
use std::thread::sleep;
use std::time::{Duration, Instant};
use bitvec::{bitarr, bits};
use bitvec::macros::internal::funty::Fundamental;
use bitvec::prelude::{Lsb0, Msb0};
use bitvec::slice::BitSlice;
use bitvec::view::BitView;
use crossbeam_channel::unbounded;

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

lazy_static! {
    static ref SPI_ROM_DATA: HashMap<u8, Vec<u8>> = {
        HashMap::from([
            (
                0x60,
                vec![
                    0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
                    0xff, 0xff, 0x03, 0xa0, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x02, 0xff, 0xff, 0xff, 0xff,
                    0xf0, 0xff, 0x89, 0x00, 0xf0, 0x01, 0x00, 0x40, 0x00, 0x40, 0x00, 0x40, 0xf9, 0xff, 0x06, 0x00,
                    0x09, 0x00, 0xe7, 0x3b, 0xe7, 0x3b, 0xe7, 0x3b, 0xff, 0xff, 0xff, 0xff, 0xff, 0xba, 0x15, 0x62,
                    0x11, 0xb8, 0x7f, 0x29, 0x06, 0x5b, 0xff, 0xe7, 0x7e, 0x0e, 0x36, 0x56, 0x9e, 0x85, 0x60, 0xff,
                    0x32, 0x32, 0x32, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
                    0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
                    0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
                    0x50, 0xfd, 0x00, 0x00, 0xc6, 0x0f, 0x0f, 0x30, 0x61, 0x96, 0x30, 0xf3, 0xd4, 0x14, 0x54, 0x41,
                    0x15, 0x54, 0xc7, 0x79, 0x9c, 0x33, 0x36, 0x63, 0x0f, 0x30, 0x61, 0x96, 0x30, 0xf3, 0xd4, 0x14,
                    0x54, 0x41, 0x15, 0x54, 0xc7, 0x79, 0x9c, 0x33, 0x36, 0x63, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
                ],
            ),
            (
                0x80,
                vec![
                    0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
                    0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
                    0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xb2, 0xa1, 0xbe, 0xff, 0x3e, 0x00, 0xf0, 0x01, 0x00, 0x40,
                    0x00, 0x40, 0x00, 0x40, 0xfe, 0xff, 0xfe, 0xff, 0x08, 0x00, 0xe7, 0x3b, 0xe7, 0x3b, 0xe7, 0x3b,
                ],
            )
        ])
    };
}

fn write(
    writable: &mut dyn Write,
    ack: u8,
    cmd: u8,
    buf: &[u8],
) -> Result<(), Box<dyn Error>> {
    let mut data = vec![ack, cmd];
    data.extend(buf);
    data.append(&mut vec![0u8; 62 - buf.len()]);
    writable.write(&data)?;

    println!("Write: {:02X?}", data);

    Ok(())
}

fn uart(
    writable: &mut dyn Write,
    input: &Input,
    count: u8,
    ack: bool,
    sub_cmd: u8,
    data: &[u8],
) -> Result<(), Box<dyn Error>> {
    let ack_byte = if ack {
        if data.len() > 0 { 0x80 | sub_cmd } else { 0x00 }
    } else {
        0x00
    };

    let mut buf = input.get_buf().to_vec();
    buf.append(&mut vec![ack_byte, sub_cmd]);
    buf.append(&mut data.to_vec());

    write(writable, 0x21, count, buf.as_slice())?;

    Ok(())
}

// fn get_input_buffer(input: &Input) -> [u8; 11] {
//     let left =
//         bit_input(input.y, 0) |
//             bit_input(input.x, 1) |
//             bit_input(input.b, 2) |
//             bit_input(input.a, 3) |
//             bit_input(input.r, 6) |
//             bit_input(input.zr, 7);
//
//     let center =
//         bit_input(input.minus, 0) |
//             bit_input(input.plus, 1) |
//             bit_input(input.stick_l.press, 2) |
//             bit_input(input.stick_r.press, 3) |
//             bit_input(input.home, 4) |
//             bit_input(input.capture, 5);
//
//     let right =
//         bit_input(input.down, 0) |
//             bit_input(input.up, 1) |
//             bit_input(input.right, 2) |
//             bit_input(input.left, 3) |
//             bit_input(input.l, 6) |
//             bit_input(input.zl, 7);
//
//     let lx = ((1.0 + input.stick_l.x) * 2047.5).floor() as u16;
//     let ly = ((1.0 + input.stick_l.y) * 2047.5).floor() as u16;
//     let rx = ((1.0 + input.stick_r.x) * 2047.5).floor() as u16;
//     let ry = ((1.0 + input.stick_r.y) * 2047.5).floor() as u16;
//
//     let left_stick = pack_shorts(lx, ly);
//     let right_stick = pack_shorts(rx, ry);
//
//     [
//         0x81,
//         left,
//         center,
//         right,
//         left_stick[0],
//         left_stick[1],
//         left_stick[2],
//         right_stick[0],
//         right_stick[1],
//         right_stick[2],
//         0x00
//     ]
// }
//
// fn bit_input(input: u8, offset: u8) -> u8 {
//     if input == 0 { 0 } else { 1 << offset }
// }
//
// fn pack_shorts(v1: u16, v2: u16) -> [u8; 3] {
//     [
//         v1.to_be_bytes()[1],
//         (v2 << 4).to_be_bytes()[0] | ((v1 >> 8) & 0x0f).to_be_bytes()[1],
//         (v2 >> 4).to_be_bytes()[1],
//     ]
// }

// trait Button {
//     type Value;
// }
//
// struct LeftButton {}
//
// impl Button for Dpad {
//     type Value = LeftButtonValue;
// }
//
// enum LeftButtonValue {
//     Y = 0,
//     X = 1,
//     B = 2,
//     A = 3,
//     R = 6,
//     ZR = 7,
// }
//
// struct CenterButton {}
//
// impl Button for CenterButton {
//     type Value = CenterButtonValue;
// }
//
// enum CenterButtonValue {
//     Minus = 0,
//     Plus = 1,
//     StickR = 2,
//     StickL = 3,
//     Home = 4,
//     Capture = 5,
// }

// fn push_button<B: Button>(input: Arc<Mutex<Input>>, button: B, value: B::Value) {
//     match button {
//         LeftButton {} => match value {
//             DpadValue::Up => {
//                 input.lock().unwrap().up = 1;
//             }
//             DpadValue::Down => {}
//             DpadValue::Left => {}
//             DpadValue::Right => {}
//         },
//         CenterButton {} => match value {
//             CenterButton::A => {}
//             CenterButton::B => {}
//             CenterButton::X => {}
//             CenterButton::Y => {}
//         },
//     }
// }

pub struct Dpad {
    pub up: u8,
    pub down: u8,
    pub left: u8,
    pub right: u8,
}

pub struct Button {
    pub a: u8,
    pub b: u8,
    pub x: u8,
    pub y: u8,
    pub l: u8,
    pub r: u8,
    pub zl: u8,
    pub zr: u8,
    pub minus: u8,
    pub plus: u8,
    pub home: u8,
    pub capture: u8,
}

pub struct Stick {
    pub x: f64,
    pub y: f64,
    pub press: bool,
}

pub struct ControllerInput {
    pub dpad: Dpad,
    pub button: Button,
    pub stick_l: Stick,
    pub stick_r: Stick,
}

pub struct Controller {
    pub path: String,
    pub fp: File,
    pub count: u8,
    pub stop_counter: Mutex<String>,
    pub stop_input: Mutex<String>,
    pub stop_communicate: Mutex<String>,
    pub input: ControllerInput,
}

struct Input {
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,

    pub a: bool,
    pub b: bool,
    pub x: bool,
    pub y: bool,

    pub l: bool,
    pub r: bool,
    pub zl: bool,
    pub zr: bool,

    pub minus: bool,
    pub plus: bool,
    pub home: bool,
    pub capture: bool,
    pub stick_l: Stick,
    pub stick_r: Stick,
}

impl Input {
    pub fn new() -> Self {
        Self {
            up: false,
            down: false,
            left: false,
            right: false,
            a: false,
            b: false,
            x: false,
            y: false,
            l: false,
            r: false,
            zl: false,
            zr: false,
            minus: false,
            plus: false,
            home: false,
            capture: false,
            stick_l: Stick {
                x: 0.0,
                y: 0.0,
                press: false,
            },
            stick_r: Stick {
                x: 0.0,
                y: 0.0,
                press: false,
            },
        }
    }

    pub fn get_buf(&self) -> [u8; 11] {
        let left =
            Self::bit_input(self.y, 0) |
                Self::bit_input(self.x, 1) |
                Self::bit_input(self.b, 2) |
                Self::bit_input(self.a, 3) |
                Self::bit_input(self.r, 6) |
                Self::bit_input(self.zr, 7);

        let center =
            Self::bit_input(self.minus, 0) |
                Self::bit_input(self.plus, 1) |
                Self::bit_input(self.stick_l.press, 2) |
                Self::bit_input(self.stick_r.press, 3) |
                Self::bit_input(self.home, 4) |
                Self::bit_input(self.capture, 5);

        let right =
            Self::bit_input(self.down, 0) |
                Self::bit_input(self.up, 1) |
                Self::bit_input(self.right, 2) |
                Self::bit_input(self.left, 3) |
                Self::bit_input(self.l, 6) |
                Self::bit_input(self.zl, 7);

        let lx = ((1.0 + self.stick_l.x) * 2047.5).floor() as u16;
        let ly = ((1.0 + self.stick_l.y) * 2047.5).floor() as u16;
        let rx = ((1.0 + self.stick_r.x) * 2047.5).floor() as u16;
        let ry = ((1.0 + self.stick_r.y) * 2047.5).floor() as u16;

        let left_stick = Self::pack_shorts(lx, ly);
        let right_stick = Self::pack_shorts(rx, ry);

        [
            0x81,
            left,
            center,
            right,
            left_stick[0],
            left_stick[1],
            left_stick[2],
            right_stick[0],
            right_stick[1],
            right_stick[2],
            0x00
        ]
    }

    fn bit_input(input: bool, offset: u32) -> u8 {
        if input { (1 as u8).checked_shl(offset).unwrap_or(0) } else { 0 }
    }

    fn pack_shorts(v1: u16, v2: u16) -> [u8; 3] {
        [
            v1.to_le_bytes()[0],
            ((v2 << 4) & 0xf0).to_le_bytes()[0] | ((v1 >> 8) & 0x0f).to_le_bytes()[0],
            (v2 >> 4).to_le_bytes()[0],
        ]
    }
}

// impl Controller {
//     pub fn uart(&mut self, ack: bool, sub_cmd: u8, data: &[u8]) -> Result<(), Box<dyn Error>> {
//         let ack_byte = if ack {
//             0x80 | if data.len() > 0 { sub_cmd } else { 0x00 }
//         } else {
//             0x00
//         };
//
//         let mut buf = self.get_input_buffer().to_vec();
//         buf.append(&mut vec![ack_byte, sub_cmd]);
//         buf.append(&mut data.to_vec());
//
//         self.write(0x21, self.count, buf.as_slice())?;
//
//         Ok(())
//     }
//
//     pub fn write(&mut self, ack: u8, cmd: u8, buf: &[u8]) -> Result<(), Box<dyn Error + '_>> {
//         let mut data = vec![ack, cmd];
//         data.extend(buf);
//         data.append(&mut vec![0u8; 62 - buf.len()]);
//         self.fp.write(&data)?;
//
//         dbg!(format!("Write: ack: {:02x}, cmd: {:02x}, buf: {:?}", ack, cmd, buf));
//
//         Ok(())
//     }
//
//     pub fn get_input_buffer(&self) -> [u8; 11] {
//         let left =
//             Controller::bit_input(self.input.button.y, 0) |
//                 Controller::bit_input(self.input.button.x, 1) |
//                 Controller::bit_input(self.input.button.b, 2) |
//                 Controller::bit_input(self.input.button.a, 3) |
//                 Controller::bit_input(self.input.button.r, 6) |
//                 Controller::bit_input(self.input.button.zr, 7);
//
//         let center =
//             Controller::bit_input(self.input.button.minus, 0) |
//                 Controller::bit_input(self.input.button.plus, 1) |
//                 Controller::bit_input(self.input.stick_l.press, 2) |
//                 Controller::bit_input(self.input.stick_r.press, 3) |
//                 Controller::bit_input(self.input.button.home, 4) |
//                 Controller::bit_input(self.input.button.capture, 5);
//
//         let right =
//             Controller::bit_input(self.input.dpad.down, 0) |
//                 Controller::bit_input(self.input.dpad.up, 1) |
//                 Controller::bit_input(self.input.dpad.right, 2) |
//                 Controller::bit_input(self.input.dpad.left, 3) |
//                 Controller::bit_input(self.input.button.l, 6) |
//                 Controller::bit_input(self.input.button.zl, 7);
//
//         let lx = ((1.0 + self.input.stick_l.x) * 2047.5).floor() as u16;
//         let ly = ((1.0 + self.input.stick_l.y) * 2047.5).floor() as u16;
//         let rx = ((1.0 + self.input.stick_r.x) * 2047.5).floor() as u16;
//         let ry = ((1.0 + self.input.stick_r.y) * 2047.5).floor() as u16;
//
//         let left_stick = Controller::pack_shorts(lx, ly);
//         let right_stick = Controller::pack_shorts(rx, ry);
//
//         [
//             0x81,
//             left,
//             center,
//             right,
//             left_stick[0],
//             left_stick[1],
//             left_stick[2],
//             right_stick[0],
//             right_stick[1],
//             right_stick[2],
//             0x00
//         ]
//     }
//
//     pub fn bit_input(input: u8, offset: u8) -> u8 {
//         if input == 0 { 0 } else { 1 << offset }
//     }
//
//     pub fn pack_shorts(v1: u16, v2: u16) -> [u8; 3] {
//         [
//             v1.to_be_bytes()[1],
//             (v2 << 4).to_be_bytes()[0] | ((v1 >> 8) & 0x0f).to_be_bytes()[1],
//             (v2 >> 4).to_be_bytes()[1],
//         ]
//     }
// }

// start input report
fn start_input_sending(
    writable: Arc<Mutex<Box<impl Write + Send + 'static>>>,
    input: Arc<Mutex<Input>>,
    count: Arc<Mutex<u8>>,
    stop_signal: Arc<Mutex<bool>>,
) {
    let interval = Duration::from_millis(30);
    let mut next = Instant::now() + interval;

    thread::spawn(move || {
        loop {
            if *stop_signal.lock().unwrap() {
                break;
            }

            write(
                &mut *writable.lock().unwrap(),
                0x30,
                *count.lock().unwrap(),
                &input.lock().unwrap().get_buf(),
            ).unwrap();

            sleep(next - Instant::now());
            next += interval;
        }
    });
}

fn start_counter(count: Arc<Mutex<u8>>, stop_signal: Arc<Mutex<bool>>) {
    thread::spawn(move || {
        loop {
            if *stop_signal.lock().unwrap() {
                break;
            }

            let mut c = count.lock().unwrap();
            *c = c.wrapping_add(1);

            sleep(Duration::from_millis(5));
        }
    });
}

fn connect<T>(
    file: Arc<Mutex<Box<T>>>,
    input: Arc<Mutex<Input>>,
    stop_signal: Arc<Mutex<bool>>,
) -> Result<(), Box<dyn Error>>
    where T: Read + Write + Send + 'static {

    // magic packet
    write(&mut *file.lock().unwrap(), 0x81, 0x03, [].as_ref())?;
    write(&mut *file.lock().unwrap(), 0x81, 0x01, [0x00, 0x03].as_ref())?;

    let counter = Arc::new(Mutex::new(0));

    start_counter(Arc::clone(&counter), Arc::clone(&stop_signal));

    thread::spawn(move || {
        println!("start communication");
        loop {
            let mut buf = [0u8; 128];
            let mut f = file.lock().unwrap();
            (*f).read(&mut buf).unwrap();

            println!("Read: {:02X?}", buf);
            match buf[0] {
                0x80 => match buf[1] {
                    0x01 => {
                        write(
                            &mut *f,
                            0x81,
                            buf[1],
                            [0x00, 0x03, 0x00, 0x00, 0x5e, 0x00, 0x53, 0x5e].as_ref(),
                        ).unwrap();
                    }
                    0x02 | 0x03 => {
                        write(&mut *f, 0x81, buf[1], [].as_ref()).unwrap();
                    }
                    0x04 => {
                        start_input_sending(
                            Arc::clone(&file),
                            Arc::clone(&input),
                            Arc::clone(&counter),
                            Arc::clone(&stop_signal),
                        );
                    }
                    0x05 => {
                        *stop_signal.lock().unwrap() = true;
                    }
                    _ => {
                        println!("Received unknown command {:X}", buf[0]);
                    }
                },
                0x01 => match buf[10] {
                    0x01 => {
                        uart(
                            &mut *f,
                            &*input.lock().unwrap(),
                            *counter.lock().unwrap(),
                            true,
                            buf[10],
                            &[0x03, 0x01],
                        ).unwrap();
                    }
                    0x02 => {
                        uart(
                            &mut *f,
                            &*input.lock().unwrap(),
                            *counter.lock().unwrap(),
                            true,
                            buf[10],
                            &[0x03, 0x48, 0x03, 0x02, 0x5e, 0x53, 0x00, 0x5e, 0x00, 0x00, 0x03, 0x01],
                        ).unwrap();
                    }
                    0x03 | 0x08 | 0x30 | 0x38 | 0x40 | 0x41 | 0x48 => {
                        uart(
                            &mut *f,
                            &*input.lock().unwrap(),
                            *counter.lock().unwrap(),
                            true,
                            buf[10],
                            &[],
                        ).unwrap();
                    }
                    0x04 => {
                        uart(
                            &mut *f,
                            &*input.lock().unwrap(),
                            *counter.lock().unwrap(),
                            true,
                            buf[10],
                            &[],
                        ).unwrap();
                    }
                    0x10 => {
                        let data = SPI_ROM_DATA.get(&buf[12]);
                        match data {
                            Some(d) => {
                                let mut uart_data = buf[11..16].to_vec();
                                uart_data.append(&mut d[usize::from(buf[11])..usize::from(buf[11] + buf[15])].to_vec());

                                uart(
                                    &mut *f,
                                    &*input.lock().unwrap(),
                                    *counter.lock().unwrap(),
                                    true,
                                    buf[10],
                                    uart_data.as_ref(),
                                ).unwrap();

                                println!("Read SPI address: {:X} {:X} {:X} {:02X?}", buf[12], buf[11], buf[15], &d[usize::from(buf[11])..usize::from(buf[11] + buf[15])])
                            }
                            None => {
                                uart(
                                    &mut *f,
                                    &*input.lock().unwrap(),
                                    *counter.lock().unwrap(),
                                    false,
                                    buf[10],
                                    &[],
                                ).unwrap();

                                println!("Unknown SPI address: {:X} {:X}", buf[12], buf[15]);
                            }
                        }
                    }
                    0x21 => {
                        uart(
                            &mut *f,
                            &*input.lock().unwrap(),
                            *counter.lock().unwrap(),
                            true,
                            buf[10],
                            &[0x01, 0x00, 0xff, 0x00, 0x03, 0x00, 0x05, 0x01],
                        ).unwrap();
                    }
                    _ => {
                        println!("UART unknown request {:X} {:02X?}", buf[10], buf);
                    }
                },
                0x00 | 0x10 | _ => {
                    println!("Unknown request {:X}", buf[0]);
                }
            }
        }
    });

    Ok(())
}

// fn main() -> () {
//     let args: Vec<String> = env::args().collect();
//     let local_ip = local_ip().unwrap();
//     let port = match args.get(1) {
//         Some(s) => s.clone(),
//         None => String::from("12351"),
//     };
//     let addr = format!("{:?}:{}", local_ip, port);
//
//     let socket = UdpSocket::bind(&addr).expect("couldn't bind socket");
//     println!("Successfully {} binding socket", &addr);
//     println!("Listening...");
//
//     let mut buff = Cursor::new([0u8; 2048]);
//
//     // csv
//     let mut wtr = WriterBuilder::new()
//         .has_headers(true)
//         .from_path("output.csv")
//         .unwrap();
//
//     let start = Instant::now();
//     loop {
//         socket
//             .recv_from(buff.get_mut())
//             .expect("didn't receive data");
//
//         match mocopi_parser::parse(buff.get_mut()) {
//             Ok(r) => match r {
//                 SkeletonOrFrame::Skeleton(s) => {
//                     dbg!(s);
//                 }
//                 SkeletonOrFrame::Frame(f) => {
//                     dbg!(f.frame.bones.first());
//                     let end = start.elapsed();
//                     for b in f.frame.bones {
//                         wtr.serialize(Row {
//                             id: &b.id.to_string().as_str(),
//                             time: format!("{}.{:03}", end.as_secs(), end.subsec_millis()).as_str(),
//                             rot_x: b.trans.rot.x,
//                             rot_y: b.trans.rot.y,
//                             rot_z: b.trans.rot.z,
//                             rot_w: b.trans.rot.w,
//                             pos_x: b.trans.pos.x,
//                             pos_y: b.trans.pos.y,
//                             pos_z: b.trans.pos.z,
//                         })
//                             .unwrap();
//                     }
//                 }
//             },
//             Err(_) => {
//                 dbg!("parse error");
//             }
//         }
//     }
// }

// fn set_input(input: &mut u8) {
//     *input += 1;
//     thread::spawn(move || {
//         sleep(Duration::from_millis(100));
//         *input -= 1;
//     });
// }

fn main() {
    let target = env::args().nth(1).unwrap();
    let file = Arc::new(Mutex::new(Box::new(File::options().read(true).write(true).open(target).unwrap())));
    let input = Arc::new(Mutex::new(Input::new()));
    let stop_signal = Arc::new(Mutex::new(false));

    connect(
        Arc::clone(&file),
        Arc::clone(&input),
        Arc::clone(&stop_signal),
    ).unwrap();

    Command::new("stty")
        .args(["-F", "/dev/tty", "cbreak", "min", "1"])
        .output()
        .unwrap();

    Command::new("stty")
        .args(["-F", "/dev/tty", "-echo"])
        .output()
        .unwrap();

    loop {
        let mut buf = [0u8; 1];
        stdin().read(&mut buf).unwrap();

        println!("pushed {}", buf[0].to_string());
        match buf[0] {
            b'w' => {
                let i = Arc::clone(&input);
                i.lock().unwrap().up = true;
                thread::spawn(move || {
                    sleep(Duration::from_millis(100));
                    i.lock().unwrap().up = false;
                });
            }
            b'a' => {
                let i = Arc::clone(&input);
                i.lock().unwrap().left = true;
                thread::spawn(move || {
                    sleep(Duration::from_millis(100));
                    i.lock().unwrap().left = true;
                });
            }
            b's' => {
                let i = Arc::clone(&input);
                i.lock().unwrap().down = true;
                thread::spawn(move || {
                    sleep(Duration::from_millis(100));
                    i.lock().unwrap().down = false;
                });
            }
            b'd' => {
                let i = Arc::clone(&input);
                i.lock().unwrap().right = true;
                thread::spawn(move || {
                    sleep(Duration::from_millis(100));
                    i.lock().unwrap().right = false;
                });
            }
            _ => {}
        };
    }

    // Command::new("stty")
    //     .args(["-F", "/dev/tty", "-cbreak"])
    //     .output()
    //     .unwrap();
    //
    // Command::new("stty")
    //     .args(["-F", "/dev/tty", "echo"])
    //     .output()
    //     .unwrap();
}
