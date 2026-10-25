[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [string]$Path
)

$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $false

if (-not $IsWindows) { throw 'Windows signing requires a Windows runner.' }
if (-not $env:WINDOWS_SIGNING_PFX_B64 -or -not $env:WINDOWS_SIGNING_PASSWORD) {
    throw 'Configure WINDOWS_SIGNING_PFX_B64 and WINDOWS_SIGNING_PASSWORD before publishing Windows packages.'
}
$file = (Resolve-Path -LiteralPath $Path).Path

# The private key stays in memory. Never change Windows certificate stores or
# put the password in a native process command line or a temporary PFX file.
$certificate = $null
$bytes = $null
try {
    try {
        $bytes = [Convert]::FromBase64String($env:WINDOWS_SIGNING_PFX_B64)
        $certificate = [Security.Cryptography.X509Certificates.X509Certificate2]::new(
            $bytes, $env:WINDOWS_SIGNING_PASSWORD,
            [Security.Cryptography.X509Certificates.X509KeyStorageFlags]::EphemeralKeySet
        )
    } catch {
        throw 'Unable to load the code-signing PFX; check the signing secrets.'
    }
    $thumbprint = $certificate.Thumbprint
    $eku = $certificate.Extensions | Where-Object { $_.Oid.Value -eq '2.5.29.37' }
    if (-not $certificate.HasPrivateKey -or
        '1.3.6.1.5.5.7.3.3' -notin $eku.EnhancedKeyUsages.Value) {
        throw 'The PFX must contain a private key and the Code Signing extended key usage.'
    }
    if ($certificate.NotAfter -le (Get-Date) -or $certificate.NotBefore -gt (Get-Date)) {
        throw 'The code-signing certificate is not currently valid.'
    }
    $selfSigned = $certificate.Subject -eq $certificate.Issuer
    if ($selfSigned -and $env:WINDOWS_SIGNING_ALLOW_SELF_SIGNED -ne 'true') {
        throw 'Self-signed code signing requires WINDOWS_SIGNING_ALLOW_SELF_SIGNED=true.'
    }

    $sdkRoots = @("${env:ProgramFiles(x86)}\Windows Kits\10\bin", "${env:ProgramFiles}\Windows Kits\10\bin")
    $architecture = [Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString().ToLowerInvariant()
    $signTool = $null
    foreach ($sdkRoot in $sdkRoots) {
        $versions = Get-ChildItem -LiteralPath $sdkRoot -Directory -ErrorAction SilentlyContinue |
            Where-Object { $_.Name -match '^\d+\.\d+\.\d+\.\d+$' } |
            Sort-Object { [version]$_.Name } -Descending
        foreach ($version in $versions) {
            foreach ($arch in @($architecture, 'x64', 'x86') | Select-Object -Unique) {
                $candidate = Join-Path $version.FullName "$arch\signtool.exe"
                if (Test-Path -LiteralPath $candidate) { $signTool = $candidate; break }
            }
            if ($signTool) { break }
        }
        if ($signTool) { break }
    }
    if (-not $signTool) { throw 'Windows SDK signtool.exe was not found.' }

    function Invoke-SignTool([string[]]$Arguments) {
        $output = & $signTool @Arguments 2>&1
        $exitCode = $LASTEXITCODE
        # SDK errors can include timestamp/CRL URLs. Never emit raw tool output.
        foreach ($line in $output) {
            Write-Output ([string]$line -replace '(?i)\b[a-z][a-z0-9+.-]*://[^\s"<>]+', '<URL>')
        }
        if ($exitCode -ne 0) { throw "SignTool failed (exit $exitCode)." }
    }

    $null = Set-AuthenticodeSignature -LiteralPath $file -Certificate $certificate -HashAlgorithm SHA256 -IncludeChain All
    Invoke-SignTool @('timestamp', '/q', '/tr', 'http://timestamp.digicert.com', '/td', 'SHA256', $file)
    if (-not ('AzurPilotWinTrust' -as [type])) { Add-Type -Path (Join-Path $PSScriptRoot 'WinTrust.cs') }
    $trustResult = [AzurPilotWinTrust]::Verify($file)
    $untrustedRoot = [Convert]::ToUInt32('800B0109', 16)
    if ($trustResult -ne 0 -and -not ($selfSigned -and $trustResult -eq $untrustedRoot)) {
        throw ('Authenticode verification failed (0x{0:X8}).' -f $trustResult)
    }
    if (-not $selfSigned) { Invoke-SignTool @('verify', '/q', '/pa', '/all', '/tw', $file) }
    $signature = Get-AuthenticodeSignature -LiteralPath $file
    if ($signature.SignerCertificate.Thumbprint -ne $thumbprint -or -not $signature.TimeStamperCertificate) {
        throw 'The output must have a signature from the configured certificate and a timestamp.'
    }
    Write-Output ("Signed and verified: {0}" -f [IO.Path]::GetFileName($file))
    if ($selfSigned) { Write-Output 'Self-signed certificate: public Windows trust is not established.' }
} finally {
    if ($certificate) { $certificate.Dispose() }
    if ($bytes) { [Array]::Clear($bytes, 0, $bytes.Length) }
}
