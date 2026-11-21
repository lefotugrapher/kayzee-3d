// Turns a numbered run of picture files into a QuickTime movie.
// usage: ridgeline-movie OUT.mov FPS FRAME.png [FRAME.png ...]
import AVFoundation
import CoreGraphics
import Foundation
import ImageIO

func fail(_ text: String) -> Never {
    FileHandle.standardError.write((text + "\n").data(using: .utf8)!)
    exit(1)
}

let args = CommandLine.arguments
guard args.count >= 4, let fps = Int32(args[2]), fps > 0 else { fail("usage: ridgeline-movie OUT.mov FPS FRAME.png [FRAME.png ...]") }
let out = URL(fileURLWithPath: args[1])
let frames = Array(args[3...])

func load(_ path: String) -> CGImage? {
    guard let src = CGImageSourceCreateWithURL(URL(fileURLWithPath: path) as CFURL, nil) else { return nil }
    return CGImageSourceCreateImageAtIndex(src, 0, nil)
}

guard let first = load(frames[0]) else { fail("cannot read \(frames[0])") }
let (w, h) = (first.width, first.height)
try? FileManager.default.removeItem(at: out)
guard let writer = try? AVAssetWriter(outputURL: out, fileType: .mov) else { fail("cannot make \(out.path)") }
// H.264 wants even sizes; other sizes are kept exact with Photo JPEG
let codec: AVVideoCodecType = (w % 2 == 0 && h % 2 == 0) ? .h264 : .jpeg
var settings: [String: Any] = [AVVideoCodecKey: codec, AVVideoWidthKey: w, AVVideoHeightKey: h]
if codec == .h264 {
    settings[AVVideoCompressionPropertiesKey] = [AVVideoAverageBitRateKey: max(2_000_000, w * h * 12)]
}
let input = AVAssetWriterInput(mediaType: .video, outputSettings: settings)
input.expectsMediaDataInRealTime = false
let adaptor = AVAssetWriterInputPixelBufferAdaptor(assetWriterInput: input, sourcePixelBufferAttributes: [
    kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_32ARGB,
    kCVPixelBufferWidthKey as String: w,
    kCVPixelBufferHeightKey as String: h,
])
writer.add(input)
guard writer.startWriting() else { fail("cannot start: \(writer.error?.localizedDescription ?? "?")") }
writer.startSession(atSourceTime: .zero)

for (k, path) in frames.enumerated() {
    guard let image = load(path) else { fail("cannot read \(path)") }
    var buffer: CVPixelBuffer?
    CVPixelBufferCreate(nil, w, h, kCVPixelFormatType_32ARGB, nil, &buffer)
    guard let pb = buffer else { fail("out of memory") }
    CVPixelBufferLockBaseAddress(pb, [])
    let ctx = CGContext(data: CVPixelBufferGetBaseAddress(pb), width: w, height: h, bitsPerComponent: 8,
                        bytesPerRow: CVPixelBufferGetBytesPerRow(pb), space: CGColorSpaceCreateDeviceRGB(),
                        bitmapInfo: CGImageAlphaInfo.noneSkipFirst.rawValue)
    ctx?.draw(image, in: CGRect(x: 0, y: 0, width: w, height: h))
    CVPixelBufferUnlockBaseAddress(pb, [])
    while !input.isReadyForMoreMediaData { usleep(2000) }
    if !adaptor.append(pb, withPresentationTime: CMTime(value: CMTimeValue(k), timescale: fps)) {
        fail("frame \(k) was refused: \(writer.error?.localizedDescription ?? "?")")
    }
}
input.markAsFinished()
writer.endSession(atSourceTime: CMTime(value: CMTimeValue(frames.count), timescale: fps))
let done = DispatchSemaphore(value: 0)
writer.finishWriting { done.signal() }
done.wait()
if writer.status != .completed { fail("the movie was not finished: \(writer.error?.localizedDescription ?? "?")") }
print("\(frames.count) frames, \(w)x\(h), \(fps) fps")
