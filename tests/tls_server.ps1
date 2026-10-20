param(
    [Parameter(Mandatory = $true)][string]$PortFile,
    [Parameter(Mandatory = $true)][string]$DerFile,
    [string]$Mode = "http",
    [int]$Connections = 1,
    [string]$PfxFile = ""
)

$ErrorActionPreference = "Stop"

try {

$rsa = [System.Security.Cryptography.RSA]::Create(2048)
$req = [System.Security.Cryptography.X509Certificates.CertificateRequest]::new(
    "CN=localhost",
    $rsa,
    [System.Security.Cryptography.HashAlgorithmName]::SHA256,
    [System.Security.Cryptography.RSASignaturePadding]::Pkcs1)
$san = [System.Security.Cryptography.X509Certificates.SubjectAlternativeNameBuilder]::new()
$san.AddDnsName("localhost")
$req.CertificateExtensions.Add($san.Build())
$cert = $req.CreateSelfSigned(
    [System.DateTimeOffset]::UtcNow.AddDays(-1),
    [System.DateTimeOffset]::UtcNow.AddDays(30))

$pfx = $cert.Export([System.Security.Cryptography.X509Certificates.X509ContentType]::Pfx, "bloom-test")
$cert2 = [System.Security.Cryptography.X509Certificates.X509Certificate2]::new(
    $pfx,
    "bloom-test",
    [System.Security.Cryptography.X509Certificates.X509KeyStorageFlags]::UserKeySet)

[System.IO.File]::WriteAllBytes($DerFile, $cert2.Export([System.Security.Cryptography.X509Certificates.X509ContentType]::Cert))
if ($PfxFile -ne "") {
    [System.IO.File]::WriteAllBytes($PfxFile, $pfx)
}

if ($Mode -eq "cert-only") {
    exit 0
}

$listener = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, 0)
$listener.Start()
[System.IO.File]::WriteAllText($PortFile, "$($listener.LocalEndpoint.Port)")

for ($c = 0; $c -lt $Connections; $c++) {
    $client = $listener.AcceptTcpClient()
    $stream = $client.GetStream()
    $ssl = [System.Net.Security.SslStream]::new($stream, $false)
    $ssl.AuthenticateAsServer($cert2, $false, [System.Security.Authentication.SslProtocols]::Tls12, $false)

    if ($Mode -eq "echo") {
        $buf = New-Object byte[] 4096
        while ($true) {
            $n = $ssl.Read($buf, 0, $buf.Length)
            if ($n -le 0) { break }
            $ssl.Write($buf, 0, $n)
            $ssl.Flush()
        }
    }
    elseif ($Mode -eq "echo-then-close") {
        $buf = New-Object byte[] 4096
        $n = $ssl.Read($buf, 0, $buf.Length)
        if ($n -gt 0) {
            $ssl.Write($buf, 0, $n)
            $ssl.Flush()
        }
        $ssl.Close()
        $client.Close()
        $listener.Stop()
        exit 0
    }
    elseif ($Mode -eq "echo-then-abort") {
        $buf = New-Object byte[] 4096
        $n = $ssl.Read($buf, 0, $buf.Length)
        if ($n -gt 0) {
            $ssl.Write($buf, 0, $n)
            $ssl.Flush()
        }
        $stream.Close()
        $client.Close()
        $listener.Stop()
        exit 0
    }
    elseif ($Mode -eq "http") {
        $reqBuf = New-Object System.Collections.Generic.List[byte]
        $buf = New-Object byte[] 1024
        $headerEnd = -1
        while ($headerEnd -lt 0) {
            $n = $ssl.Read($buf, 0, $buf.Length)
            if ($n -le 0) { break }
            for ($i = 0; $i -lt $n; $i++) { $reqBuf.Add($buf[$i]) }
            $text = [System.Text.Encoding]::ASCII.GetString($reqBuf.ToArray())
            $headerEnd = $text.IndexOf("`r`n`r`n")
        }
        $body = "hallo über TLS"
        $bytes = [System.Text.Encoding]::UTF8.GetBytes($body)
        $head = "HTTP/1.1 200 OK`r`nContent-Type: text/plain; charset=utf-8`r`nContent-Length: $($bytes.Length)`r`nConnection: close`r`n`r`n"
        $headBytes = [System.Text.Encoding]::ASCII.GetBytes($head)
        $ssl.Write($headBytes, 0, $headBytes.Length)
        $ssl.Write($bytes, 0, $bytes.Length)
        $ssl.Flush()
    }
    elseif ($Mode -eq "http-chunked") {
        $reqBuf = New-Object System.Collections.Generic.List[byte]
        $buf = New-Object byte[] 1024
        $headerEnd = -1
        while ($headerEnd -lt 0) {
            $n = $ssl.Read($buf, 0, $buf.Length)
            if ($n -le 0) { break }
            for ($i = 0; $i -lt $n; $i++) { $reqBuf.Add($buf[$i]) }
            $text = [System.Text.Encoding]::ASCII.GetString($reqBuf.ToArray())
            $headerEnd = $text.IndexOf("`r`n`r`n")
        }
        $head = "HTTP/1.1 200 OK`r`nContent-Type: text/plain`r`nTransfer-Encoding: chunked`r`nConnection: close`r`n`r`n"
        $headBytes = [System.Text.Encoding]::ASCII.GetBytes($head)
        $ssl.Write($headBytes, 0, $headBytes.Length)
        $chunks = @("5`r`nfirst`r`n", "7`r`n second`r`n", "0`r`nX-Trailer: yes`r`n`r`n")
        foreach ($chunk in $chunks) {
            $cb = [System.Text.Encoding]::ASCII.GetBytes($chunk)
            $ssl.Write($cb, 0, $cb.Length)
        }
        $ssl.Flush()
    }

    $ssl.Close()
    $client.Close()
}

$listener.Stop()

} catch {
    $msg = $_.Exception.ToString()
    if ($_.Exception.InnerException) { $msg += "`nINNER: " + $_.Exception.InnerException.ToString() }
    [System.IO.File]::WriteAllText("$PortFile.err", $msg)
    exit 1
}
