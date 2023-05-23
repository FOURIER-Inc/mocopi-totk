#[macro_use]
extern crate lazy_static;

use std::cell::Ref;
use std::collections::HashMap;
use csv::WriterBuilder;
use local_ip_address::local_ip;
use mocopi_parser::SkeletonOrFrame;
use serde::Serialize;
use std::{env, thread};
use std::error::Error;
use std::fs::File;
use std::io::{Cursor, Read, Write};
use std::net::UdpSocket;
use std::sync::{Arc, Mutex, MutexGuard};
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

trait Button {
    type Value;
}

struct Dpad;

enum DpadValue {
    Up,
    Down,
    Left,
    Right,
}

impl Button for Dpad {
    type Value = DpadValue;
}

struct ActionButton; // 適当に命名

enum ActionButtonValue {
    A,
    B,
    X,
    Y,
}

impl Button for ActionButton {
    type Value = ActionButtonValue;
}

struct SideButton;

enum SideButtonValue {
    L,
    R,
    ZL,
    ZR,
}

impl Button for SideButton {
    type Value = SideButtonValue;
}

struct ControlButton;

enum ControlButtonValue {
    Minus,
    Plus,
    Home,
    Capture,
}

impl Button for ControlButton {
    type Value = ControlButtonValue;
}

struct StickPressButton;

enum StickPressButtonValue {
    Left,
    Right,
}

impl Button for StickPressButton {
    type Value = StickPressButtonValue;
}

fn press_button<B: Button>(inputs: Arc<Mutex<Input>>, button: B, value: B::Value) {
    let inputs = Arc::clone(&inputs).lock()?;
    *inputs.buttons.push(value);
}

enum StickType {
    Left,
    Right,
}

fn tilt_stick(stick_type: StickType, x: f64, y: f64) {

}

fn write(fp: &mut dyn Write, ack: u8, cmd: u8, buf: &[u8]) -> Result<(), Box<dyn Error + '_>> {
    let mut data = vec![ack, cmd];
    data.extend(buf);
    data.append(&mut vec![0u8; 62 - buf.len()]);
    fp.write(&data)?;

    dbg!(format!("Write: ack: {:02x}, cmd: {:02x}, buf: {:?}", ack, cmd, buf));

    Ok(())
}

fn uart(&mut self, ack: bool, sub_cmd: u8, data: &[u8]) -> Result<(), Box<dyn Error + '_>> {
    let ack_byte = if ack {
        0x80 | if data.len() > 0 { sub_cmd } else { 0x00 }
    } else {
        0x00
    };

    let mut buf = self.get_input_buffer().to_vec();
    buf.append(&mut vec![ack_byte, sub_cmd]);
    buf.append(&mut data.to_vec());

    self.write(0x21, self.count, buf.as_slice())?;

    Ok(())
}

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
    pub press: u8,
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
    pub buttons: Vec<Button>,
    pub stick_l: Stick,
    pub stick_r: Stick,
}

fn test() {
    let i = Arc::new(Mutex::new(Input {
        buttons: vec![],
        stick_l: Stick {
            x: 0.0,
            y: 0.0,
            press: 0,
        },
        stick_r: Stick {
            x: 0.0,
            y: 0.0,
            press: 0,
        },
    }));
}

impl Controller {
    pub fn uart(&mut self, ack: bool, sub_cmd: u8, data: &[u8]) -> Result<(), Box<dyn Error + '_>> {
        let ack_byte = if ack {
            0x80 | if data.len() > 0 { sub_cmd } else { 0x00 }
        } else {
            0x00
        };

        let mut buf = self.get_input_buffer().to_vec();
        buf.append(&mut vec![ack_byte, sub_cmd]);
        buf.append(&mut data.to_vec());

        self.write(0x21, self.count, buf.as_slice())?;

        Ok(())
    }

    pub fn write(&mut self, ack: u8, cmd: u8, buf: &[u8]) -> Result<(), Box<dyn Error + '_>> {
        let mut data = vec![ack, cmd];
        data.extend(buf);
        data.append(&mut vec![0u8; 62 - buf.len()]);
        self.fp.write(&data)?;

        dbg!(format!("Write: ack: {:02x}, cmd: {:02x}, buf: {:?}", ack, cmd, buf));

        Ok(())
    }

    pub fn get_input_buffer(&self) -> [u8; 11] {
        let left =
            Controller::bit_input(self.input.button.y, 0) |
                Controller::bit_input(self.input.button.x, 1) |
                Controller::bit_input(self.input.button.b, 2) |
                Controller::bit_input(self.input.button.a, 3) |
                Controller::bit_input(self.input.button.r, 6) |
                Controller::bit_input(self.input.button.zr, 7);

        let center =
            Controller::bit_input(self.input.button.minus, 0) |
                Controller::bit_input(self.input.button.plus, 1) |
                Controller::bit_input(self.input.stick_l.press, 2) |
                Controller::bit_input(self.input.stick_r.press, 3) |
                Controller::bit_input(self.input.button.home, 4) |
                Controller::bit_input(self.input.button.capture, 5);

        let right =
            Controller::bit_input(self.input.dpad.down, 0) |
                Controller::bit_input(self.input.dpad.up, 1) |
                Controller::bit_input(self.input.dpad.right, 2) |
                Controller::bit_input(self.input.dpad.left, 3) |
                Controller::bit_input(self.input.button.l, 6) |
                Controller::bit_input(self.input.button.zl, 7);

        let lx = ((1.0 + self.input.stick_l.x) * 2047.5).floor() as u16;
        let ly = ((1.0 + self.input.stick_l.y) * 2047.5).floor() as u16;
        let rx = ((1.0 + self.input.stick_r.x) * 2047.5).floor() as u16;
        let ry = ((1.0 + self.input.stick_r.y) * 2047.5).floor() as u16;

        let left_stick = Controller::pack_shorts(lx, ly);
        let right_stick = Controller::pack_shorts(rx, ry);

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

    pub fn bit_input(input: u8, offset: u8) -> u8 {
        if input == 0 { 0 } else { 1 << offset }
    }

    pub fn pack_shorts(v1: u16, v2: u16) -> [u8; 3] {
        [
            v1.to_be_bytes()[1],
            (v2 << 4).to_be_bytes()[0] | ((v1 >> 8) & 0x0f).to_be_bytes()[1],
            (v2 >> 4).to_be_bytes()[1],
        ]
    }
}

fn connect(mut c: Controller) -> Result<(), Box<dyn Error>> {
    // magic packet
    c.write(0x81, 0x03, [].as_ref())?;
    c.write(0x81, 0x01, [0x00, 0x03].as_ref())?;

    thread::spawn(move || {
        let mut buf = [0u8; 128];

        loop {
            c.fp.read(&mut buf).unwrap();

            match buf[0] {
                0x80 => match buf[1] {
                    0x01 => {
                        c.write(
                            0x81,
                            buf[1],
                            [0x00, 0x03, 0x00, 0x00, 0x5e, 0x00, 0x53, 0x5e].as_ref(),
                        ).unwrap();
                    }
                    0x02 | 0x03 => {
                        c.write(0x81, buf[1], [].as_ref()).unwrap();
                    }
                    0x04 => {}
                    0x05 => {}
                    _ => {
                        dbg!("received unknown command", buf[0]);
                    }
                },
                0x01 => match buf[10] {
                    0x01 => {
                        c.uart(true, buf[10], &[0x03, 0x01]).unwrap();
                    }
                    0x02 => {
                        c.uart(true, buf[10], &[
                            0x03, 0x48, 0x03,
                            0x02, 0x5e, 0x53, 0x00, 0x5e, 0x00, 0x00, 0x03, 0x01,
                        ]).unwrap();
                    }
                    0x03 | 0x08 | 0x30 | 0x38 | 0x40 | 0x41 | 0x48 => {
                        c.uart(
                            true,
                            buf[10],
                            &[],
                        ).unwrap();
                    }
                    0x04 => {
                        c.uart(true, buf[10], &[]).unwrap();
                    }
                    0x10 => {
                        let data = SPI_ROM_DATA.get(&buf[12]);
                        match data {
                            Some(d) => {
                                let mut uart_data = buf[11..16].to_vec();
                                uart_data.append(&mut d[usize::from(buf[11])..usize::from(buf[11] + buf[15])].to_vec());

                                c.uart(true, buf[10], uart_data.as_ref()).unwrap();
                            }
                            None => {
                                c.uart(false, buf[10], &[]).unwrap();
                            }
                        }
                    }
                    0x21 => {
                        c.uart(true, buf[10], &[0x01, 0x00, 0xff, 0x00, 0x03, 0x00, 0x05, 0x01]).unwrap();
                    }
                    _ => {
                        dbg!("received unknown command", buf[0]);
                    }
                },
                0x00 | 0x10 | _ => {
                    dbg!("unknown request", buf[0]);
                }
            }
        }
    });

    Ok(())
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
