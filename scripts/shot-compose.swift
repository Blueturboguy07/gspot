// Composite only the chosen windows onto a dark backdrop and write a PNG.
// ScreenCaptureKit renders exactly the windows in the filter, in the window
// server's z-order, so nothing else on the desktop can land in the shot.
//   shot-compose out.png x y w h pid[,pid...] [pid:x,y,w,h]
// The listed pids contribute every on-screen window that touches the region;
// the optional last argument adds one process's window only when its frame
// matches (the TextEdit draft, so a person's other TextEdit documents stay out).
import Cocoa
import ScreenCaptureKit

let a = CommandLine.arguments
guard a.count >= 7 else { fputs("usage: shot-compose out.png x y w h pid[,pid...] [pid:x,y,w,h]\n", stderr); exit(2) }
let out = a[1]
let region = CGRect(x: Double(a[2])!, y: Double(a[3])!, width: Double(a[4])!, height: Double(a[5])!)
let pids = Set(a[6].split(separator: ",").compactMap { pid_t($0) })
var matchPid: pid_t? = nil
var matchRect = CGRect.zero
if a.count >= 8 {
  let parts = a[7].split(separator: ":")
  matchPid = pid_t(parts[0])
  let n = parts[1].split(separator: ",").map { Double($0)! }
  matchRect = CGRect(x: n[0], y: n[1], width: n[2], height: n[3])
}

let sem = DispatchSemaphore(value: 0)
var exitCode: Int32 = 0
Task.detached {
  do {
    let content = try await SCShareableContent.excludingDesktopWindows(true, onScreenWindowsOnly: true)
    guard let display = content.displays.first(where: { $0.frame.intersects(region) }) ?? content.displays.first else {
      throw NSError(domain: "shot", code: 1, userInfo: [NSLocalizedDescriptionKey: "no display"])
    }
    var picked: [SCWindow] = []
    for w in content.windows {
      guard let pid = w.owningApplication?.processID else { continue }
      let r = w.frame
      if !r.intersects(region) { continue }
      if pids.contains(pid) { picked.append(w); continue }
      if let mp = matchPid, pid == mp,
         abs(r.minX - matchRect.minX) < 6, abs(r.minY - matchRect.minY) < 60, abs(r.width - matchRect.width) < 6 {
        picked.append(w)
      }
    }
    for w in picked {
      fputs("window \(w.windowID) pid \(w.owningApplication?.processID ?? 0) layer \(w.windowLayer) \(Int(w.frame.minX)),\(Int(w.frame.minY)) \(Int(w.frame.width))x\(Int(w.frame.height)) \(w.title ?? "")\n", stderr)
    }
    guard !picked.isEmpty else { throw NSError(domain: "shot", code: 2, userInfo: [NSLocalizedDescriptionKey: "no windows matched"]) }
    let filter = SCContentFilter(display: display, including: picked)
    let scale = Double(filter.pointPixelScale)
    let cfg = SCStreamConfiguration()
    cfg.sourceRect = CGRect(x: region.minX - display.frame.minX, y: region.minY - display.frame.minY, width: region.width, height: region.height)
    cfg.width = Int(region.width * scale)
    cfg.height = Int(region.height * scale)
    cfg.showsCursor = false
    cfg.captureResolution = .best
    cfg.pixelFormat = kCVPixelFormatType_32BGRA
    // backgroundColor stays at its default (clear); the property is `assign`, so a
    // freshly made CGColor set here is released at once and the capture crashes.
    let shot = try await SCScreenshotManager.captureImage(contentFilter: filter, configuration: cfg)
    let W = shot.width, H = shot.height
    let cs = CGColorSpace(name: CGColorSpace.sRGB)!
    guard let ctx = CGContext(data: nil, width: W, height: H, bitsPerComponent: 8, bytesPerRow: 0, space: cs,
                              bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue) else {
      throw NSError(domain: "shot", code: 3, userInfo: [NSLocalizedDescriptionKey: "no context"])
    }
    let colors = [CGColor(red: 0.17, green: 0.21, blue: 0.24, alpha: 1), CGColor(red: 0.06, green: 0.08, blue: 0.09, alpha: 1)] as CFArray
    let grad = CGGradient(colorsSpace: cs, colors: colors, locations: [0, 1])!
    ctx.drawLinearGradient(grad, start: CGPoint(x: 0, y: H), end: CGPoint(x: W, y: 0), options: [])
    ctx.draw(shot, in: CGRect(x: 0, y: 0, width: W, height: H))
    guard let img = ctx.makeImage() else { throw NSError(domain: "shot", code: 4, userInfo: [NSLocalizedDescriptionKey: "no image"]) }
    let rep = NSBitmapImageRep(cgImage: img)
    guard let png = rep.representation(using: .png, properties: [:]) else { throw NSError(domain: "shot", code: 5, userInfo: [NSLocalizedDescriptionKey: "no png"]) }
    try png.write(to: URL(fileURLWithPath: out))
    print("composed \(W)x\(H) from \(picked.count) windows")
  } catch {
    fputs("shot-compose: \(error.localizedDescription)\n", stderr)
    exitCode = 1
  }
  sem.signal()
}
sem.wait()
exit(exitCode)
