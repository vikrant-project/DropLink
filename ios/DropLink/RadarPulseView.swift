import SwiftUI

public struct RadarPulseView: View {
    @State private var isPulsing = false
    
    public init() {}
    
    public var body: some View {
        ZStack {
            Circle()
                .stroke(Color(red: 0.39, green: 0.40, blue: 0.95).opacity(0.3), lineWidth: 2)
                .frame(width: 90, height: 90)
                .scaleEffect(isPulsing ? 1.4 : 0.8)
                .opacity(isPulsing ? 0 : 1)
                .animation(
                    Animation.easeOut(duration: 1.8).repeatForever(autoreverses: false),
                    value: isPulsing
                )
            
            Circle()
                .fill(Color(red: 0.07, green: 0.09, blue: 0.15))
                .frame(width: 64, height: 64)
                .overlay(
                    Circle().stroke(Color(red: 0.31, green: 0.27, blue: 0.90), lineWidth: 2)
                )
            
            Image(systemName: "wifi")
                .font(.system(size: 24, weight: .semibold))
                .foregroundColor(Color(red: 0.51, green: 0.55, blue: 0.97))
        }
        .onAppear {
            isPulsing = true
        }
    }
}
