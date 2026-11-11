// 功能：只读取指定子进程的原生窗口信息，不枚举其他应用内容；时间：2026-11-11；作者：lq。
import Foundation
import CoreGraphics

let targetPID = Int32(CommandLine.arguments[1])!
let windows = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID) as? [[String: Any]] ?? []
let ownWindows = windows.filter { ($0[kCGWindowOwnerPID as String] as? Int32) == targetPID }
let result = ownWindows.map { info -> [String: Any] in
    return ["id": info[kCGWindowNumber as String] ?? 0,
            "title": info[kCGWindowName as String] ?? "unavailable",
            "bounds": info[kCGWindowBounds as String] ?? [:],
            "layer": info[kCGWindowLayer as String] ?? -1]
}
let data = try JSONSerialization.data(withJSONObject: result, options: [.sortedKeys])
print(String(data: data, encoding: .utf8)!)
