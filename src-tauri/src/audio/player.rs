use rodio::{Decoder, OutputStream, OutputStreamBuilder, Sink};
use std::{fs::File, io::BufReader};

pub struct AudioPlayer {
    // must be kept alive
    _stream: OutputStream,
    sink: Sink,
}

impl AudioPlayer {
    pub fn new() -> Self {
        // Open the system audio output
        let stream =
            OutputStreamBuilder::open_default_stream().expect("failed to open audio output");

        // Create a sink connected to the stream's mixer
        let sink = Sink::connect_new(&stream.mixer());

        Self {
            _stream: stream,
            sink,
        }
    }

    pub fn play(&self, path: String) {
        let file = File::open(path).expect("failed to open audio file");
        let reader = BufReader::new(file);

        let source = Decoder::try_from(reader).expect("failed to decode audio");

        self.sink.stop(); // stop previous audio
        self.sink.append(source);
        self.sink.play();
    }

    pub fn pause(&self) {
        self.sink.pause();
    }

    pub fn stop(&self) {
        self.sink.stop();
    }
}
