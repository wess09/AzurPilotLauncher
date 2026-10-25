using System;
using System.Runtime.InteropServices;

// Return the exact Authenticode policy result. Self-signed builds may accept
// CERT_E_UNTRUSTEDROOT only; digest, signature, expiry and timestamp errors fail.
public static class AzurPilotWinTrust
{
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    private struct FileInfo
    {
        public uint Size;
        [MarshalAs(UnmanagedType.LPWStr)] public string Path;
        public IntPtr File;
        public IntPtr Subject;
    }

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    private struct TrustData
    {
        public uint Size;
        public IntPtr PolicyCallback;
        public IntPtr SipClient;
        public uint UiChoice;
        public uint RevocationChecks;
        public uint UnionChoice;
        public IntPtr FileInfo;
        public uint StateAction;
        public IntPtr StateData;
        public IntPtr UrlReference;
        public uint ProviderFlags;
        public uint UiContext;
        public IntPtr SignatureSettings;
    }

    [DllImport("wintrust.dll", ExactSpelling = true)]
    private static extern uint WinVerifyTrust(IntPtr window, ref Guid action, ref TrustData data);

    public static uint Verify(string path)
    {
        var file = new FileInfo { Size = (uint)Marshal.SizeOf<FileInfo>(), Path = path };
        IntPtr filePointer = Marshal.AllocHGlobal(Marshal.SizeOf<FileInfo>());
        Marshal.StructureToPtr(file, filePointer, false);
        try
        {
            var data = new TrustData
            {
                Size = (uint)Marshal.SizeOf<TrustData>(),
                UiChoice = 2, // WTD_UI_NONE: never prompt on a CI runner.
                UnionChoice = 1, // WTD_CHOICE_FILE
                FileInfo = filePointer
            };
            var action = new Guid("00AAC56B-CD44-11d0-8CC2-00C04FC295EE");
            return WinVerifyTrust(new IntPtr(-1), ref action, ref data);
        }
        finally
        {
            Marshal.DestroyStructure<FileInfo>(filePointer);
            Marshal.FreeHGlobal(filePointer);
        }
    }
}
