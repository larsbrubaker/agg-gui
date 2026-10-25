// AudioWorklet processor for agg-gui-whistle's microphone (src/web/mic.rs,
// loaded from Config::worklet_url): posts each render quantum's first channel
// (mono f32, at the AudioContext's rate) to the main thread, which measures
// the level, batches ~250 ms and sends it to the transcription worker.
// At a stop the main thread posts "flush" and waits for "flushed": messages
// on a port arrive in order, so every quantum posted before the answer is in
// by then (src/web/drain.rs). Plain JS because worklets load by URL.
class AggGuiWhistleCapture extends AudioWorkletProcessor {
  constructor() {
    super();
    this.port.onmessage = (event) => {
      if (event.data === "flush") this.port.postMessage("flushed");
    };
  }

  process(inputs) {
    const channel = inputs[0] && inputs[0][0];
    if (channel && channel.length > 0) {
      this.port.postMessage(channel.slice(0));
    }
    return true;
  }
}

registerProcessor("agg-gui-whistle-capture", AggGuiWhistleCapture);
