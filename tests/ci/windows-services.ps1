# Kiln's services scenario on a GitHub Actions Windows runner (ADR 0039 §3, P3-13): snapshot,
# services added and exercised, uninstall, snapshot again, compare. No desktop session and no
# Herd here: those are checked by hand in the Windows VM before a release.
#   windows-services.ps1 -Kiln <path to kiln.exe>
# Reuses the VM scenario's helpers (lib.ps1) and snapshot (snapshot.ps1). The runner's image
# may run its own MySQL or PostgreSQL, so the checks read the port Kiln chose instead of
# assuming the usual one: another service on it is a coexistence check for free.
param(
    [Parameter(Mandatory)] [string] $Kiln,
    # How the run ends: `kiln uninstall --keep-data` or `--delete-data`.
    [ValidateSet('keep-data', 'delete-data')] [string] $Uninstall = 'keep-data'
)
$ErrorActionPreference = 'Continue'
$dir = Join-Path $env:RUNNER_TEMP 'kiln-ci'
$log = Join-Path $dir 'scenario.log'
$status = Join-Path $dir 'status'
$vm = Join-Path $PSScriptRoot '..\vm\windows'
New-Item -ItemType Directory -Force $dir | Out-Null
Set-Content $log '' -Encoding utf8
$script:failed = $false
. (Join-Path $vm 'lib.ps1')

# Everything Say writes goes to the job's output as well.
$sayToFile = ${function:Say}
function Say($text) { & $sayToFile $text; Write-Host $text }

& (Join-Path $vm 'snapshot.ps1') | Set-Content (Join-Path $dir 'before.txt') -Encoding utf8
if (Select-String -Path (Join-Path $dir 'before.txt') -Pattern 'Kiln present' -Quiet) {
    Say 'FAIL Kiln is already set up here'; exit 2
}
Say "== runner: $((Get-CimInstance Win32_OperatingSystem).Caption)"
Get-NetTCPConnection -State Listen -ErrorAction SilentlyContinue |
    Where-Object { $_.LocalPort -in 1025, 3306, 3307, 5432, 6379, 8025 } |
    ForEach-Object { Say "     already listening: $($_.LocalAddress):$($_.LocalPort) (PID $($_.OwningProcess))" }

# The port an add printed: "running as <name>: <role> 127.0.0.1:<port>".
function PortOf($out, $role) { [int][regex]::Match($out, "$role 127\.0\.0\.1:(\d+)").Groups[1].Value }

Say '== Mailpit'
$script:smtp = 0; $script:http = 0
Check 'service add mailpit: downloaded, checked, running on loopback' {
    $out = Kiln service add mailpit
    Expect 'running as mailpit-1' $out
    $script:smtp = PortOf $out 'smtp'; $script:http = PortOf $out 'http'
    if (-not $script:smtp -or -not $script:http) { throw "no ports in:`n$out" }
    Expect "mailpit.test proxies to http://127.0.0.1:$($script:http)" $out   # its inbox as a site (ADR 0044)
}
function MailTotal { (Invoke-RestMethod "http://127.0.0.1:$($script:http)/api/v1/messages" -TimeoutSec 5).total }
Check 'a mail sent over SMTP is caught (Mailpit''s API counts 1)' {
    Send-MailMessage -SmtpServer 127.0.0.1 -Port $script:smtp -From app@example.test -To you@example.test `
        -Subject 'from CI' -Body 'hello from kiln' -WarningAction SilentlyContinue
    if ((MailTotal) -ne 1) { throw "Mailpit has $(MailTotal) message(s)" }
}
Check 'service stop closes its ports; start brings it back with its mail' {
    Kiln service stop mailpit-1 | Out-Null
    if (Test-NetConnection 127.0.0.1 -Port $script:http -InformationLevel Quiet -WarningAction SilentlyContinue) {
        throw 'still answering after stop'
    }
    Kiln service start mailpit-1 | Out-Null
    if ((MailTotal) -ne 1) { throw 'the caught mail is gone after a restart' }
}

Say '== Ports next to other programs (ADR 0037)'
Check 'a second instance moves off the first one''s ports and says whose they are' {
    # With --json, as the app runs it: the steps after the download show (P4C-01).
    $out = Kiln --json service add mailpit --name mail-two
    Expect '{"text":"Setting up its data folder","type":"step"}' $out
    Expect '{"text":"Starting mail-two","type":"step"}' $out
    Expect "used by Kiln's mailpit-1" $out
    Expect 'mailpit.test is already taken' $out
    Kiln service remove mail-two --delete-data | Out-Null
    if ((Kiln sites) -notmatch 'mailpit.test') { throw 'removing mail-two took mailpit-1''s site' }
}
Kiln service stop mailpit-1 | Out-Null
$squatter = Start-Process powershell.exe -WindowStyle Hidden -PassThru -ArgumentList '-NoProfile', '-Command',
    "`$l = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback, $($script:smtp)); `$l.Start(); Start-Sleep 300"
Start-Sleep -Seconds 2
Check 'service start refuses a port another program took, naming it' {
    $out = (& $Kiln service start mailpit-1 2>&1 | ForEach-Object { "$_" }) -join "`n"
    if ($LASTEXITCODE -eq 0) { throw "it started:`n$out" }
    if ($out -notmatch "mailpit-1 can't start: port $($script:smtp) \(smtp\) is used by .*powershell.exe") { throw "no holder named:`n$out" }
    $global:LASTEXITCODE = 0
}
Check 'kiln doctor warns about the instance' {
    $out = (& $Kiln doctor 2>&1 | ForEach-Object { "$_" }) -join "`n"
    if ($out -notmatch "mailpit-1 can't run: port $($script:smtp) \(smtp\)") { throw "no warning:`n$out" }
    $global:LASTEXITCODE = 0
}
Stop-Process -Id $squatter.Id -Force -ErrorAction SilentlyContinue
Start-Sleep -Seconds 1
Check 'service start works once the other program is gone' {
    Kiln service start mailpit-1 | Out-Null
    if ((MailTotal) -ne 1) { throw 'Mailpit is not back with its mail' }
}

Say '== MySQL and MariaDB (ADR 0038)'
function Sql($instance, $statement) { ($statement | & $Kiln db $instance 2>&1 | ForEach-Object { "$_" }) -join "`n" }
$script:mysqlPort = 0
Check 'service add mysql --version 8.4: from Oracle, set up and running on loopback' {
    $out = Kiln service add mysql --version 8.4
    Expect 'running as mysql-8.4' $out
    $script:mysqlPort = PortOf $out 'mysql'
}
Check 'service add mariadb --version 11.8: beside it, on another port' {
    $out = Kiln service add mariadb --version 11.8
    Expect 'running as mariadb-11.8' $out
    if ((PortOf $out 'mysql') -eq $script:mysqlPort) { throw "same port as mysql-8.4:`n$out" }
}
Check 'only loopback listens on their ports (no X plugin on 33060)' {
    $ours = (Kiln service list) -split "`n" | ForEach-Object { [regex]::Match($_, '127\.0\.0\.1:(\d+)').Groups[1].Value } | Where-Object { $_ }
    $wide = Get-NetTCPConnection -State Listen | Where-Object {
        "$($_.LocalPort)" -in $ours -and $_.LocalAddress -ne '127.0.0.1' -and
        (Get-Process -Id $_.OwningProcess -ErrorAction SilentlyContinue).ProcessName -like 'kiln-*'
    }
    if ($wide) { throw "listening beyond loopback: $(($wide | ForEach-Object { "$($_.LocalAddress):$($_.LocalPort)" }) -join ', ')" }
    if (Get-NetTCPConnection -State Listen -LocalPort 33060 -ErrorAction SilentlyContinue | Where-Object { (Get-Process -Id $_.OwningProcess).ProcessName -eq 'kiln-mysqld' }) {
        throw 'the X plugin is listening'
    }
}
foreach ($instance in 'mysql-8.4', 'mariadb-11.8') {
    Check "kiln db $instance`: a database, a table, a row" {
        $out = Sql $instance "CREATE DATABASE kiln_ci; CREATE TABLE kiln_ci.t (v VARCHAR(20)); INSERT INTO kiln_ci.t VALUES ('kept'); SELECT v FROM kiln_ci.t;"
        Expect 'kept' $out
    }
}
Check 'data survives a restart' {
    Kiln service restart mysql-8.4 | Out-Null
    Expect 'kept' (Sql 'mysql-8.4' 'SELECT v FROM kiln_ci.t;')
}
Check 'a stop shuts it down cleanly (its own log says so), and start brings it back' {
    Kiln service stop mariadb-11.8 | Out-Null
    $out = Kiln service logs mariadb-11.8 --lines 8
    if ($out -notmatch 'Shutdown complete') { throw "no clean shutdown in:`n$out" }
    Kiln service start mariadb-11.8 | Out-Null
    Expect 'kept' (Sql 'mariadb-11.8' 'SELECT v FROM kiln_ci.t;')
}

Say '== PostgreSQL (ADR 0040)'
function Psql($instance, $database, $statement) {
    ($statement | & $Kiln db $instance $database 2>&1 | ForEach-Object { "$_" }) -join "`n"
}
$script:pgPort = 0
# The runner runs elevated, which PostgreSQL refuses: this passes only if services run
# without administrator rights (ADR 0040 §3).
Check 'service add postgres --version 17: from EDB, set up and running on loopback' {
    $out = Kiln service add postgres --version 17
    Expect 'running as postgres-17' $out
    $script:pgPort = PortOf $out 'postgres'
}
Check 'service add postgres (18): beside it, on another port' {
    $out = Kiln service add postgres
    Expect 'running as postgres-18' $out
    if ((PortOf $out 'postgres') -eq $script:pgPort) { throw "same port as postgres-17:`n$out" }
}
Check 'PostgreSQL listens on 127.0.0.1 only and runs without administrator rights' {
    $listening = Get-NetTCPConnection -State Listen | Where-Object {
        (Get-Process -Id $_.OwningProcess -ErrorAction SilentlyContinue).ProcessName -eq 'kiln-postgres'
    }
    if (-not $listening) { throw 'kiln-postgres listens nowhere' }
    $wide = $listening | Where-Object LocalAddress -ne '127.0.0.1'
    if ($wide) { throw "listening beyond loopback: $(($wide | ForEach-Object { "$($_.LocalAddress):$($_.LocalPort)" }) -join ', ')" }
}
foreach ($instance in 'postgres-17', 'postgres-18') {
    Check "kiln db $instance`: a database, a table, a row" {
        Psql $instance $null 'CREATE DATABASE kiln_ci;' | Out-Null
        $out = Psql $instance 'kiln_ci' "CREATE TABLE t (v text); INSERT INTO t VALUES ('kept'); SELECT v FROM t;"
        Expect 'kept' $out
    }
}
Check 'PostgreSQL data survives a restart' {
    Kiln service restart postgres-18 | Out-Null
    Expect 'kept' (Psql 'postgres-18' 'kiln_ci' 'SELECT v FROM t;')
}
Check 'a stop shuts PostgreSQL down cleanly (its log says so), and start brings it back' {
    Kiln service stop postgres-17 | Out-Null
    $out = Kiln service logs postgres-17 --lines 8
    if ($out -notmatch 'database system is shut down') { throw "no clean shutdown in:`n$out" }
    Kiln service start postgres-17 | Out-Null
    Expect 'kept' (Psql 'postgres-17' 'kiln_ci' 'SELECT v FROM t;')
}

Say '== phpMyAdmin and Adminer (ADR 0043)'
Check 'php install 8.5, web install and web start (plain HTTP, for the tools)' {
    Kiln php install 8.5 | Out-Null
    Kiln web install | Out-Null
    $script:web = Kiln web start
}
$script:pma = ''; $script:adminer = ''
Check 'tool add phpmyadmin: from phpmyadmin.net, served as pma.test' {
    $out = Kiln tool add phpmyadmin
    Expect 'Linked pma.test' $out
    $script:pma = [regex]::Match($out, 'URL: (\S+)').Groups[1].Value
    if (-not $script:pma) { throw "no URL in:`n$out" }
}
Check 'tool add adminer: from GitHub, served as adminer.test' {
    $out = Kiln tool add adminer
    $script:adminer = [regex]::Match($out, 'URL: (\S+)').Groups[1].Value
    if (-not $script:adminer) { throw "no URL in:`n$out" }
}
# The hosts file isn't touched in CI: curl resolves the sites' names to 127.0.0.1 itself.
$jar = Join-Path $dir 'cookies.txt'
function Page($url, [string[]] $more = @()) {
    $uri = [Uri] $url
    curl.exe -sS -L --max-time 60 -c $jar -b $jar --resolve "$($uri.Host):$($uri.Port):127.0.0.1" @more $url 2>&1 | Out-String
}
function Clean($page) {
    if ($page -match '(Deprecated|Warning|Fatal error|Notice): ') { throw "PHP reported a problem:`n$($page.Substring([Math]::Max(0, $page.IndexOf($Matches[0]) - 200), 600))" }
}
Check 'phpMyAdmin opens logged in, with Kiln''s MySQL and MariaDB instances, on PHP 8.5' {
    $page = Page $script:pma
    Clean $page
    foreach ($name in 'mysql-8.4', 'mariadb-11.8') { Expect $name $page }
    if ($page -match 'name="pma_password"') { throw 'it shows a login form' }
    $databases = Page "$($script:pma)index.php?route=/server/databases&server=1"
    Clean $databases
    Expect 'kiln_ci' $databases
}
Check 'Adminer lists Kiln''s instances and logs in to PostgreSQL without a password' {
    Remove-Item $jar -ErrorAction SilentlyContinue
    $form = Page $script:adminer
    Clean $form
    foreach ($name in 'mysql-8.4', 'mariadb-11.8', 'postgres-18') { Expect $name $form }
    # As a browser does: the login form's CSRF token goes back with it.
    $token = [regex]::Match($form, "name='token' value='([^']+)'").Groups[1].Value
    if (-not $token) { throw 'no token in the login form' }
    $page = Page $script:adminer @('--data-urlencode', 'auth[server]=postgres-18', '--data-urlencode', 'auth[db]=',
        '--data-urlencode', "token=$token")
    Clean $page
    if ($page -notmatch 'kiln_ci') {
        $heading = [regex]::Match($page, '<h2>(.*?)</h2>').Groups[1].Value
        $error = [regex]::Match($page, "class='error'>(.*?)</div>").Groups[1].Value
        throw "no kiln_ci; heading: $heading; error: $error"
    }
}
# Kiln's web server answers on the port in the tools' URL (80, or the next free one).
function SiteBase($name) { $uri = [Uri] $script:pma; "http://$name.test:$($uri.Port)/" }
Check 'mailpit.test serves the inbox through Kiln''s web server, with its mail' {
    $page = Page (SiteBase 'mailpit')
    Expect 'Mailpit' $page
    $api = Page "$(SiteBase 'mailpit')api/v1/messages" | ConvertFrom-Json
    if ($api.total -ne 1) { throw "the inbox through mailpit.test has $($api.total) message(s)" }
}
Check 'service port mailpit-1 http=8125: mailpit.test follows it' {
    Expect 'mailpit.test now proxies to http://127.0.0.1:8125' (Kiln service port mailpit-1 http=8125)
    $script:http = 8125
    $api = Page "$(SiteBase 'mailpit')api/v1/messages" | ConvertFrom-Json
    if ($api.total -ne 1) { throw "the inbox through mailpit.test has $($api.total) message(s)" }
}
# HTTPS with Kiln's CA. A runner can't answer Windows' trust dialog, so curl is given Kiln's
# root certificate instead: the chain the browser would check, without trusting it.
Check 'https://mailpit.test: the inbox over HTTPS, with a certificate from Kiln''s CA' {
    Kiln ca init | Out-Null
    Kiln secure mailpit | Out-Null
    $url = [regex]::Match((Kiln sites), 'https://mailpit\.test[^\s]*').Value
    if (-not $url) { throw "no HTTPS URL in:`n$(Kiln sites)" }
    $uri = [Uri] $url
    $page = curl.exe -sS --max-time 60 --ssl-revoke-best-effort --cacert "$env:APPDATA\Kiln\ca\root.crt" `
        --resolve "$($uri.Host):$($uri.Port):127.0.0.1" "$($url.TrimEnd('/'))/api/v1/messages" 2>&1 | Out-String
    if ($LASTEXITCODE -ne 0) { throw "curl couldn't verify or reach it:`n$page" }
    if (($page | ConvertFrom-Json).total -ne 1) { throw "the inbox over HTTPS answered:`n$page" }
}
Say '== Redis (ADR 0041)'
# A client's commands go through files, not a pipe: Windows PowerShell puts a byte-order mark
# in front of what it pipes to a program, and redis-cli reads it as part of the first command.
function Client($instance, $database, $commands) {
    $in = Join-Path $dir 'client-in.txt'; $out = Join-Path $dir 'client-out.txt'
    [IO.File]::WriteAllText($in, $commands + "`n")
    $arguments = @('db', $instance) + @($database | Where-Object { $null -ne $_ })
    Start-Process $Kiln -ArgumentList $arguments -RedirectStandardInput $in -RedirectStandardOutput $out `
        -NoNewWindow -Wait
    [IO.File]::ReadAllText($out)
}
$script:redisPort = 0
Check 'service add redis: from redis-windows, running on loopback' {
    $out = Kiln service add redis
    Expect 'running as redis-8.10' $out
    $script:redisPort = PortOf $out 'redis'
}
Check 'service add redis --version 8.8: beside it, on another port' {
    $out = Kiln service add redis --version 8.8
    Expect 'running as redis-8.8' $out
    if ((PortOf $out 'redis') -eq $script:redisPort) { throw "same port as redis-8.10:`n$out" }
}
Check 'Redis listens on 127.0.0.1 only' {
    $listening = Get-NetTCPConnection -State Listen | Where-Object {
        (Get-Process -Id $_.OwningProcess -ErrorAction SilentlyContinue).ProcessName -eq 'kiln-redis'
    }
    if (@($listening).Count -lt 2) { throw "kiln-redis listens on $(@($listening).Count) port(s)" }
    $wide = $listening | Where-Object LocalAddress -ne '127.0.0.1'
    if ($wide) { throw "listening beyond loopback: $(($wide | ForEach-Object { "$($_.LocalAddress):$($_.LocalPort)" }) -join ', ')" }
}
Check 'kiln db redis-8.10: keys in database 0 and 1 stay apart' {
    Client 'redis-8.10' $null 'SET where zero' | Out-Null
    Client 'redis-8.10' 1 'SET where one' | Out-Null
    Expect 'zero' (Client 'redis-8.10' $null 'GET where')
    Expect 'one' (Client 'redis-8.10' 1 'GET where')
}
# Laravel's cache (database 1) uses Lua scripts; Garnet ran them in database 0 (I-75).
Check 'a Lua script run in database 1 acts on database 1' {
    Client 'redis-8.10' 1 "EVAL `"return redis.call('SET', KEYS[1], ARGV[1])`" 1 lua kept" | Out-Null
    Expect 'kept' (Client 'redis-8.10' 1 'GET lua')
    $zero = Client 'redis-8.10' $null 'EXISTS lua'
    if ($zero.Trim() -ne '0') { throw "the script wrote to database 0: $zero" }
}
Check 'Redis data survives a restart (saved at shutdown)' {
    Kiln service restart redis-8.10 | Out-Null
    Expect 'kept' (Client 'redis-8.10' 1 'GET lua')
}
Check 'a stop shuts Redis down cleanly (its log says so)' {
    Kiln service stop redis-8.8 | Out-Null
    $out = Kiln service logs redis-8.8 --lines 8
    if ($out -notmatch 'ready to exit, bye bye') { throw "no clean shutdown in:`n$out" }
    Kiln service start redis-8.8 | Out-Null
}

# Whether only 127.0.0.1 listens for the program `name`, on at least `count` ports.
function OnlyLoopback($name, $count) {
    $listening = Get-NetTCPConnection -State Listen | Where-Object {
        (Get-Process -Id $_.OwningProcess -ErrorAction SilentlyContinue).ProcessName -eq $name
    }
    if (@($listening).Count -lt $count) { throw "$name listens on $(@($listening).Count) port(s)" }
    $wide = $listening | Where-Object LocalAddress -ne '127.0.0.1'
    if ($wide) { throw "listening beyond loopback: $(($wide | ForEach-Object { "$($_.LocalAddress):$($_.LocalPort)" }) -join ', ')" }
}

Say '== Meilisearch (ADR 0042)'
$script:meili = 0
Check 'service add meilisearch: from GitHub, running on loopback' {
    $out = Kiln service add meilisearch
    Expect 'running as meilisearch-1' $out
    $script:meili = PortOf $out 'http'
}
Check 'Meilisearch listens on 127.0.0.1 only' { OnlyLoopback 'kiln-meilisearch' 1 }
function Meili($method, $path, $body) {
    $params = @{ Method = $method; Uri = "http://127.0.0.1:$($script:meili)$path"; TimeoutSec = 10 }
    if ($body) { $params.Body = $body; $params.ContentType = 'application/json' }
    Invoke-RestMethod @params
}
function MeiliTask($task) {
    $deadline = (Get-Date).AddSeconds(30)
    do {
        Start-Sleep -Milliseconds 300
        $state = (Meili GET "/tasks/$($task.taskUid)").status
    } while ($state -in 'enqueued', 'processing' -and (Get-Date) -lt $deadline)
    if ($state -ne 'succeeded') { throw "task $($task.taskUid): $state" }
}
Check 'a document is indexed and found' {
    MeiliTask (Meili POST '/indexes/books/documents' '[{"id": 1, "title": "kept by kiln"}]')
    $hits = (Meili POST '/indexes/books/search' '{"q": "kept"}').hits
    if ($hits.Count -ne 1 -or $hits[0].title -ne 'kept by kiln') { throw "hits: $($hits | ConvertTo-Json -Compress)" }
}
Check 'Meilisearch data survives a restart' {
    Kiln service restart meilisearch-1 | Out-Null
    $hits = (Meili POST '/indexes/books/search' '{"q": "kept"}').hits
    if ($hits.Count -ne 1) { throw "hits after the restart: $($hits.Count)" }
}

Say '== RustFS (ADR 0042)'
$script:s3 = 0; $script:console = 0
Check 'service add rustfs: from GitHub, running on loopback' {
    $out = Kiln service add rustfs
    Expect 'running as rustfs-1' $out
    $script:s3 = PortOf $out 's3'; $script:console = PortOf $out 'console'
}
Check 'RustFS listens on 127.0.0.1 only (S3 and console)' { OnlyLoopback 'kiln-rustfs' 2 }
# One S3 request, signed with AWS Signature Version 4 and the credentials Kiln sets.
function S3($method, $path, [byte[]] $body = @()) {
    $sha = [Security.Cryptography.SHA256]::Create()
    $hex = { param($bytes) -join ($bytes | ForEach-Object { $_.ToString('x2') }) }
    $hmac = { param([byte[]] $key, $text) ([Security.Cryptography.HMACSHA256]::new($key)).ComputeHash([Text.Encoding]::UTF8.GetBytes($text)) }
    $now = [DateTime]::UtcNow; $date = $now.ToString('yyyyMMdd'); $stamp = $now.ToString("yyyyMMdd'T'HHmmss'Z'")
    $hostPort = "127.0.0.1:$($script:s3)"; $payload = & $hex $sha.ComputeHash($body)
    $canonical = "$method`n$path`n`nhost:$hostPort`nx-amz-content-sha256:$payload`nx-amz-date:$stamp`n`nhost;x-amz-content-sha256;x-amz-date`n$payload"
    $scope = "$date/us-east-1/s3/aws4_request"
    $toSign = "AWS4-HMAC-SHA256`n$stamp`n$scope`n$(& $hex $sha.ComputeHash([Text.Encoding]::UTF8.GetBytes($canonical)))"
    $key = [Text.Encoding]::UTF8.GetBytes('AWS4rustfsadmin')
    foreach ($part in $date, 'us-east-1', 's3', 'aws4_request') { $key = & $hmac $key $part }
    $signature = & $hex (& $hmac $key $toSign)
    $headers = @{
        'x-amz-date' = $stamp; 'x-amz-content-sha256' = $payload
        Authorization = "AWS4-HMAC-SHA256 Credential=rustfsadmin/$scope, SignedHeaders=host;x-amz-content-sha256;x-amz-date, Signature=$signature"
    }
    $params = @{ Method = $method; Uri = "http://$hostPort$path"; Headers = $headers; TimeoutSec = 10; UseBasicParsing = $true }
    if ($body.Length) { $params.Body = $body }
    Invoke-WebRequest @params
}
# An object's text: Windows PowerShell gives a text response as a string, anything else as bytes.
function S3Text($path) {
    $content = (S3 GET $path).Content
    if ($content -is [byte[]]) { [Text.Encoding]::UTF8.GetString($content) } else { $content }
}
Check 'a bucket is made, an object stored and read back' {
    S3 PUT '/kiln-ci' | Out-Null
    S3 PUT '/kiln-ci/hello.txt' ([Text.Encoding]::UTF8.GetBytes('kept by kiln')) | Out-Null
    $read = S3Text '/kiln-ci/hello.txt'
    if ($read -ne 'kept by kiln') { throw "read back: $read" }
}
Check 'the console answers' {
    $status = (Invoke-WebRequest "http://127.0.0.1:$($script:console)/" -UseBasicParsing -TimeoutSec 10).StatusCode
    if ($status -ne 200) { throw "status $status" }
}
Check 'RustFS data survives a restart' {
    Kiln service restart rustfs-1 | Out-Null
    $read = S3Text '/kiln-ci/hello.txt'
    if ($read -ne 'kept by kiln') { throw "read back: $read" }
}

Say '== MongoDB (ADR 0042)'
Check 'service add mongodb: from MongoDB, with mongosh, running on loopback' {
    $out = Kiln service add mongodb
    Expect 'running as mongodb-9' $out
    Expect 'mongosh-2.14.1-win32-x64.zip' $out
}
Check 'MongoDB listens on 127.0.0.1 only' { OnlyLoopback 'kiln-mongod' 1 }
Check 'kiln db mongodb-9: a document in, the same document out' {
    $out = Client 'mongodb-9' 'kiln_ci' "db.t.insertOne({ v: 'kept by kiln' }); print('found: ' + db.t.findOne().v)"
    Expect 'found: kept by kiln' $out
}
Check 'a stop shuts MongoDB down through mongosh (its log says so), and its data survives' {
    Kiln service stop mongodb-9 | Out-Null
    $out = Kiln service logs mongodb-9 --lines 30
    if ($out -notmatch 'Now exiting') { throw "no clean shutdown in:`n$out" }
    Kiln service start mongodb-9 | Out-Null
    Expect 'found: kept by kiln' (Client 'mongodb-9' 'kiln_ci' "print('found: ' + db.t.findOne().v)")
}
Check 'mongosh left nothing outside Kiln''s folders and sent no usage data' {
    foreach ($d in "$env:APPDATA\mongodb", "$env:LOCALAPPDATA\mongodb") {
        if (Test-Path $d) { throw "$d exists" }
    }
}

Say '== A Laravel app wired to Kiln''s services (ADR 0045)'
# Outside $dir, which is uploaded after the run: the app's vendor folder stays out of it.
$projects = Join-Path $env:RUNNER_TEMP 'projects'
New-Item -ItemType Directory -Force $projects | Out-Null
$shop = Join-Path $projects 'shop'
$envFile = Join-Path $shop '.env'
function Php { $exe = Join-Path (Get-ChildItem "$env:LOCALAPPDATA\Kiln\php" -Directory | Select-Object -First 1).FullName 'php.exe'; & $exe @args 2>&1 | Out-String }
function EnvHash { (Get-FileHash $envFile -Algorithm SHA256).Hash }
Check 'kiln new shop --db mysql-8.4: created, its .env pointed at MySQL, migrated there' {
    $out = Kiln new shop --path $projects --db mysql-8.4
    Expect 'Created the database shop on mysql-8.4' $out
    Expect 'now points at mysql-8.4' $out
    $count = Client 'mysql-8.4' 'shop' 'SELECT COUNT(*) FROM migrations;'
    if ($count -notmatch '(?m)^\s*[1-9]') { throw "no migrations on mysql-8.4:`n$count" }
}
# Laravel keeps its sessions in the database by default, so a page load reads and writes it.
function ShopPage {
    $page = Page (SiteBase 'shop')
    Clean $page
    if ($page -notmatch 'Laravel') { throw "no Laravel page:`n$($page.Substring(0, [Math]::Min(600, $page.Length)))" }
}
Check 'the app serves pages on MySQL 8.4' { ShopPage }
foreach ($database in 'mariadb-11.8', 'postgres-18') {
    Check "the app migrates and serves pages on $database" {
        Kiln env use $database --project $shop --yes | Out-Null
        Push-Location $shop
        try { $out = Php artisan migrate --force } finally { Pop-Location }
        if ($LASTEXITCODE -ne 0) { throw "migrate failed:`n$out" }
        ShopPage
        $count = Client $database 'shop' 'SELECT COUNT(*) FROM sessions;'
        if ($count -notmatch '[1-9]') { throw "no session row in $database`:`n$count" }
    }
}
Check 'back on MySQL 8.4' {
    Kiln env use mysql-8.4 --project $shop --yes | Out-Null
    ShopPage
}
$script:beforeRedis = EnvHash
$script:redisOp = ''; $script:mailOp = ''
Check 'kiln env use redis-8.10 --cache --session --queue: old and new values shown, then journaled' {
    $out = Kiln env use redis-8.10 --project $shop --cache --session --queue --yes
    Expect 'REDIS_CLIENT' $out
    Expect 'composer require predis/predis' $out
    $script:redisOp = [regex]::Match($out, 'kiln undo (\d+)').Groups[1].Value
    if (-not $script:redisOp) { throw "no undo number in:`n$out" }
}
Check 'kiln env use mailpit-1' {
    $out = Kiln env use mailpit-1 --project $shop --yes
    $script:mailOp = [regex]::Match($out, 'kiln undo (\d+)').Groups[1].Value
    $text = Get-Content $envFile -Raw
    if ($text -notmatch "(?m)^MAIL_PORT=$($script:smtp)\r?$") { throw "MAIL_PORT isn't $($script:smtp)" }
    if ($text -notmatch '(?m)^CACHE_STORE=redis\r?$') { throw 'CACHE_STORE isn''t redis' }
}
Check 'composer require predis/predis' {
    Push-Location $shop
    try { Php "$env:LOCALAPPDATA\Kiln\bin\composer.phar" require predis/predis --no-interaction }
    finally { Pop-Location }
}
Check 'Laravel''s cache, locks, tags, sessions, queues and limiters on Kiln''s Redis (spike 03''s checks)' {
    New-Item -ItemType Directory -Force (Join-Path $shop 'app\Console\Commands') | Out-Null
    Copy-Item (Join-Path $PSScriptRoot '..\..\spikes\windows-cache\KilnSpike.php') (Join-Path $shop 'app\Console\Commands\KilnSpike.php')
    Push-Location $shop
    try { $out = Php artisan kiln:spike } finally { Pop-Location }
    if ($LASTEXITCODE -ne 0 -or $out -notmatch 'PASSED') { throw "the checks failed:`n$out" }
}
Check 'a mail sent by Laravel lands in Kiln''s Mailpit' {
    Set-Content (Join-Path $shop 'kiln-mail.php') -Encoding ascii -Value @'
<?php
require __DIR__ . '/vendor/autoload.php';
$app = require __DIR__ . '/bootstrap/app.php';
$app->make(Illuminate\Contracts\Console\Kernel::class)->bootstrap();
Illuminate\Support\Facades\Mail::raw('hello from shop', fn ($m) => $m->to('you@example.test')->subject('from shop'));
echo "sent\n";
'@
    Push-Location $shop
    try { Expect 'sent' (Php kiln-mail.php) } finally { Pop-Location }
    if ((MailTotal) -ne 2) { throw "Mailpit has $(MailTotal) message(s)" }
}
Check 'kiln undo puts the .env back byte for byte' {
    Kiln undo $script:mailOp --yes | Out-Null
    Kiln undo $script:redisOp --yes | Out-Null
    if ((EnvHash) -ne $script:beforeRedis) { throw 'the .env differs from before the wiring' }
}
Check 'kiln history lists the wiring that is still in effect' { Expect 'DB_CONNECTION' (Kiln history) }
Say '== Project services: kiln.toml [services] and herd.yml (ADR 0046)'
Check 'kiln import herd maps herd.yml''s services into kiln.toml' {
    Set-Content (Join-Path $shop 'herd.yml') -Encoding ascii -Value @'
services:
    mysql:
        version: 8.0.36
        port: '${DB_PORT}'
    redis:
        version: 7.0.0
    minio:
        version: '2024'
    mariadb:
        version: 12.3
'@
    $out = Kiln import herd $shop
    Expect 'services: mysql 8.0, redis 7.0, rustfs 2024, mariadb 12.3' $out
    Expect 'minio became rustfs' $out
    Expect '[services]' (Get-Content (Join-Path $shop 'kiln.toml') -Raw)
}
Check 'kiln services up in the project: reuses, adds, starts and wires what kiln.toml lists' {
    $out = Kiln services up --project $shop --yes
    Expect 'Kiln has no MySQL 8.0; using 8.4.13' $out
    Expect 'mysql: using mysql-8.4' $out
    Expect 'redis: using redis-8.10' $out
    Expect 'rustfs: using rustfs-1' $out
    Expect 'running as mariadb-12.3' $out
    Expect 'mariadb-12.3 is a second database' $out
    $text = Get-Content $envFile -Raw
    foreach ($line in 'DB_CONNECTION=mysql', "REDIS_PORT=$($script:redisPort)", "AWS_ENDPOINT=http://127.0.0.1:$($script:s3)") {
        if ($text -notmatch "(?m)^$([regex]::Escape($line))\r?$") { throw "$line isn't in the .env" }
    }
}

Say '== Laravel Reverb in the project (ADR 0047)'
Check 'composer require laravel/reverb -W' {
    Push-Location $shop
    # -W: Laravel 13 locks guzzlehttp/psr7 3.x, and Reverb 1.12 still needs 2.x (ADR 0047).
    try { $out = Php "$env:LOCALAPPDATA\Kiln\bin\composer.phar" require laravel/reverb -W --no-interaction }
    finally { Pop-Location }
    if ($LASTEXITCODE -ne 0) {
        throw (($out -split "`n" | Where-Object { $_ -match 'Problem|^\s+- ' } | Select-Object -First 8) -join "`n")
    }
}
$script:reverb = 0
Check 'kiln service add reverb --project shop: php artisan reverb:start, kept up by Kiln, on loopback' {
    # The app's own keys, as `php artisan install:broadcasting --reverb` writes them.
    Add-Content $envFile "`nREVERB_APP_ID=kiln`nREVERB_APP_KEY=kiln-key`nREVERB_APP_SECRET=kiln-secret`n" -Encoding ascii
    $out = Kiln service add reverb --project $shop
    Expect 'running as reverb-shop' $out
    $script:reverb = PortOf $out 'reverb'
    if (-not $script:reverb) { throw "no port in:`n$out" }
    OnlyLoopback 'php' 1
}
Check 'kiln env use reverb-shop: its project by default, BROADCAST_CONNECTION and REVERB_*' {
    Kiln env use reverb-shop --yes | Out-Null
    $text = Get-Content $envFile -Raw
    foreach ($line in 'BROADCAST_CONNECTION=reverb', "REVERB_PORT=$($script:reverb)", 'REVERB_HOST=127.0.0.1', 'REVERB_APP_KEY=kiln-key') {
        if ($text -notmatch "(?m)^$([regex]::Escape($line))\r?$") { throw "$line isn't in the .env" }
    }
}
# A browser's opening WebSocket request; the server keeps the connection, so curl times out.
function Handshake {
    $answer = curl.exe -s -i --max-time 3 -H 'Connection: Upgrade' -H 'Upgrade: websocket' `
        -H 'Sec-WebSocket-Version: 13' -H 'Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==' `
        "http://127.0.0.1:$($script:reverb)/app/kiln-key" 2>&1 | Out-String
    $global:LASTEXITCODE = 0
    $answer
}
Check 'a browser''s WebSocket handshake is accepted' { Expect '101 Switching Protocols' (Handshake) }
Check 'Laravel broadcasts an event through it (a signed request Reverb checks)' {
    Set-Content (Join-Path $shop 'kiln-broadcast.php') -Encoding ascii -Value @'
<?php
require __DIR__ . '/vendor/autoload.php';
$app = require __DIR__ . '/bootstrap/app.php';
$app->make(Illuminate\Contracts\Console\Kernel::class)->bootstrap();
Illuminate\Support\Facades\Broadcast::connection('reverb')->broadcast(['kiln'], 'hello', ['from' => 'ci']);
echo "kiln-broadcast-sent\n";
'@
    Push-Location $shop
    try { Expect 'kiln-broadcast-sent' (Php kiln-broadcast.php) } finally { Pop-Location }
}
Check 'service restart reverb-shop: it comes back' {
    Kiln service restart reverb-shop | Out-Null
    Expect '101 Switching Protocols' (Handshake)
}

Say '== Dev processes (ADR 0060)'
function QueueProcesses { @(Get-CimInstance Win32_Process -Filter "Name='php.exe'" | Where-Object { $_.CommandLine -match 'queue:(listen|work)' }) }
Check 'kiln service add queue --project shop: php artisan queue:listen, kept up by Kiln' {
    Expect 'running as queue-shop' (Kiln service add queue --project $shop)
    $deadline = (Get-Date).AddSeconds(10)
    while ((QueueProcesses).Count -lt 1 -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 250 }
    if ((QueueProcesses).Count -lt 1) { throw "no queue:listen process:`n$(Kiln service logs queue-shop)" }
}
Check 'a job dispatched by Laravel runs on it (the queue is Kiln''s Redis)' {
    $done = Join-Path $shop 'storage\kiln-queued.txt'
    Remove-Item $done -ErrorAction SilentlyContinue
    Set-Content (Join-Path $shop 'kiln-dispatch.php') -Encoding ascii -Value @'
<?php
require __DIR__ . '/vendor/autoload.php';
$app = require __DIR__ . '/bootstrap/app.php';
$app->make(Illuminate\Contracts\Console\Kernel::class)->bootstrap();
dispatch(function () { file_put_contents(storage_path('kiln-queued.txt'), 'ran'); });
echo 'kiln-dispatched on ' . config('queue.default') . "\n";
'@
    Push-Location $shop
    try { $sent = Php kiln-dispatch.php } finally { Pop-Location }
    Expect 'kiln-dispatched' $sent
    # On the sync queue the script would run the job itself: the worker must be the one.
    if ($sent -match 'kiln-dispatched on sync') { throw "the queue is sync:`n$sent" }
    $deadline = (Get-Date).AddSeconds(60)
    while (-not (Test-Path $done) -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 500 }
    if (-not (Test-Path $done)) { throw "the job didn't run within 60 s:`n$(Kiln service logs queue-shop)" }
    # The worker logs DONE just after the job's own write: wait for the line too.
    $deadline = (Get-Date).AddSeconds(15)
    while ($true) {
        $workerLog = Kiln service logs queue-shop
        if ($workerLog -match 'DONE' -or (Get-Date) -gt $deadline) { break }
        Start-Sleep -Milliseconds 500
    }
    Expect 'DONE' $workerLog
    # The app finds it by the site's folder, exactly as `sites` gives it (ADR 0060 §6).
    $site = ((& $Kiln --json sites | Select-Object -Last 1 | ConvertFrom-Json).data.sites | Where-Object { $_.host -eq 'shop.test' }).project
    $inst = ((& $Kiln --json service list | Select-Object -Last 1 | ConvertFrom-Json).data.instances | Where-Object { $_.name -eq 'queue-shop' }).project
    if ($site -ne $inst) { throw "site folder $site, instance folder $inst" }
}
Check 'service stop queue-shop: no queue process is left (its own process tree is ended)' {
    Kiln service stop queue-shop | Out-Null
    $left = QueueProcesses
    if ($left.Count) { throw "still running:`n$($left.CommandLine -join "`n")" }
}
Check 'kiln service add scheduler --project shop: php artisan schedule:work' {
    Expect 'running as scheduler-shop' (Kiln service add scheduler --project $shop)
    $deadline = (Get-Date).AddSeconds(30)
    do { Start-Sleep -Milliseconds 500; $log = Kiln service logs scheduler-shop } until ($log -match 'scheduled tasks' -or (Get-Date) -gt $deadline)
    Expect 'scheduled tasks' $log
}
Check 'kiln service add vite without node_modules: refused, naming npm install' {
    $out = (& $Kiln service add vite --project $shop 2>&1 | Out-String)
    if ($LASTEXITCODE -eq 0 -or $out -notmatch 'npm install') { throw "expected a refusal naming npm install:`n$out" }
    $global:LASTEXITCODE = 0
}
Check 'dump capture: dump() in a request goes to Kiln''s dump server, not the page (ADR 0062)' {
    Expect 'running as dumps-shop' (Kiln service add dumps --project $shop)
    $wired = Kiln env use dumps-shop --yes
    $op = [regex]::Match($wired, 'kiln undo (\d+)').Groups[1].Value
    Expect 'VAR_DUMPER_FORMAT=server' (Get-Content $envFile -Raw)
    Add-Content (Join-Path $shop 'routes\web.php') -Encoding ascii -Value "`nRoute::get('/kiln-dump', function () { dump(['kiln' => 'dumped-by-ci']); return 'after-the-dump'; });"
    $page = Page "$(SiteBase 'shop')kiln-dump"
    Expect 'after-the-dump' $page
    if ($page -match 'dumped-by-ci') { throw "the dump stayed in the page:`n$page" }
    $deadline = (Get-Date).AddSeconds(15)
    do { Start-Sleep -Milliseconds 500; $log = Kiln service logs dumps-shop } until ($log -match 'dumped-by-ci' -or (Get-Date) -gt $deadline)
    Expect 'dumped-by-ci' $log
    if ($op) { Kiln undo $op --yes | Out-Null }
}
Check 'the dev processes are removed; the project stays' {
    foreach ($name in 'queue-shop', 'scheduler-shop', 'dumps-shop') { Kiln service remove $name | Out-Null }
    if (-not (Test-Path (Join-Path $shop 'artisan'))) { throw 'the project lost its artisan' }
}

Say '== PECL extensions (ADR 0067)'
Check 'every pinned PECL build matches its SHA-256 on downloads.php.net' {
    $pins = Get-Content (Join-Path $PSScriptRoot '..\..\crates\kiln-php\src\pecl.rs') -Raw
    $found = [regex]::Matches($pins, 'archive: "(php_redis-([0-9.]+)-[^"]+)",\s*sha256: "([0-9a-f]{64})"')
    if ($found.Count -lt 4) { throw "found $($found.Count) pins" }
    foreach ($pin in $found) {
        $file = Join-Path $dir $pin.Groups[1].Value
        curl.exe -fsSL --max-time 120 -o $file "https://downloads.php.net/~windows/pecl/releases/redis/$($pin.Groups[2].Value)/$($pin.Groups[1].Value)"
        if ($LASTEXITCODE -ne 0) { throw "couldn't download $($pin.Groups[1].Value)" }
        $sum = (Get-FileHash $file -Algorithm SHA256).Hash.ToLower()
        if ($sum -ne $pin.Groups[3].Value) { throw "$($pin.Groups[1].Value): $sum, pinned $($pin.Groups[3].Value)" }
    }
}
Check 'kiln php ext add redis: checked, placed, turned on; phpredis talks to Kiln''s Redis' {
    Expect 'redis is now on' (Kiln php ext add redis)
    $port = ((& $Kiln --json service list | Select-Object -Last 1 | ConvertFrom-Json).data.instances | Where-Object { $_.name -eq 'redis-8.10' }).main_port
    $script = Join-Path $dir 'phpredis.php'
    Set-Content $script -Encoding ascii -Value "<?php `$r = new Redis(); `$r->connect('127.0.0.1', $port); `$r->set('kiln-phpredis', 'ok'); echo `$r->get('kiln-phpredis');"
    Expect 'ok' (Php $script)
    Expect 'redis' ((Php -m) -join "`n")
}

Say '== Xdebug on demand (ADR 0061)'
Check 'every pinned Xdebug build matches its SHA-256 on xdebug.org' {
    $pins = Get-Content (Join-Path $PSScriptRoot '..\..\crates\kiln-php\src\xdebug.rs') -Raw
    $found = [regex]::Matches($pins, 'file: "(php_xdebug-[^"]+)",\s*sha256: "([0-9a-f]{64})"')
    if ($found.Count -lt 4) { throw "found $($found.Count) pins" }
    foreach ($pin in $found) {
        $file = Join-Path $dir $pin.Groups[1].Value
        # curl: Windows PowerShell's web client rejects xdebug.org's response line.
        curl.exe -fsSL --max-time 120 -o $file "https://xdebug.org/files/$($pin.Groups[1].Value)"
        if ($LASTEXITCODE -ne 0) { throw "couldn't download $($pin.Groups[1].Value)" }
        $sum = (Get-FileHash $file -Algorithm SHA256).Hash.ToLower()
        if ($sum -ne $pin.Groups[2].Value) { throw "$($pin.Groups[1].Value): $sum, pinned $($pin.Groups[2].Value)" }
    }
}
Set-Content (Join-Path $shop 'public\kiln-xdebug.php') -Encoding ascii -Value "<?php echo extension_loaded('xdebug') ? 'debug' : 'plain';"
$xdebugUrl = "$(SiteBase 'shop')kiln-xdebug.php"
function XdebugPage([string[]] $more = @()) {
    $uri = [Uri] $xdebugUrl
    (curl.exe -sS --max-time 30 --resolve "$($uri.Host):$($uri.Port):127.0.0.1" @more $xdebugUrl 2>&1 | Out-String).Trim()
}
Check 'kiln php xdebug on: fetched, checked, loaded by a debug worker of its own' {
    Expect 'is on for PHP' (Kiln php xdebug on)
    $list = (Kiln --json php list) -split "`n" | Where-Object { $_ } | Select-Object -Last 1 | ConvertFrom-Json
    if (-not ($list.data.versions | Where-Object { $_.xdebug })) { throw "no version has xdebug on: $($list | ConvertTo-Json -Depth 4)" }
}
Check 'a request without a trigger stays on the plain workers' {
    $page = XdebugPage
    if ($page -ne 'plain') { throw "got: $page" }
}
Check 'the XDEBUG_SESSION cookie and ?XDEBUG_SESSION_START reach the debug worker' {
    $cookie = XdebugPage @('-H', 'Cookie: other=1; XDEBUG_SESSION=kiln')
    if ($cookie -notmatch 'debug\s*$') { throw "with the cookie: $cookie" }
    $uri = [Uri] $xdebugUrl
    $query = (curl.exe -sS --max-time 30 --resolve "$($uri.Host):$($uri.Port):127.0.0.1" "$($xdebugUrl)?XDEBUG_SESSION_START=1" 2>&1 | Out-String).Trim()
    if ($query -notmatch 'debug\s*$') { throw "with the query: $query" }
    # A cookie whose value only mentions the name isn't a trigger.
    $other = XdebugPage @('-H', 'Cookie: note=XDEBUG_SESSION=kiln')
    if ($other -ne 'plain') { throw "with another cookie: $other" }
}
Check 'Xdebug connects to an editor listening on 127.0.0.1:9003 (its init packet arrives)' {
    $listener = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, 9003)
    $listener.Start()
    try {
        $out = Join-Path $dir 'xdebug-page.txt'
        $uri = [Uri] $xdebugUrl
        $curl = Start-Process curl.exe -NoNewWindow -PassThru -RedirectStandardOutput $out `
            -ArgumentList @('-sS', '--max-time', '30', '--resolve', "$($uri.Host):$($uri.Port):127.0.0.1", '-H', '"Cookie: XDEBUG_SESSION=kiln"', $xdebugUrl)
        $accept = $listener.AcceptTcpClientAsync()
        if (-not $accept.Wait(20000)) { throw 'Xdebug never connected' }
        $client = $accept.Result
        $client.ReceiveTimeout = 10000
        $buffer = New-Object byte[] 4096
        $read = $client.GetStream().Read($buffer, 0, $buffer.Length)
        $init = [System.Text.Encoding]::UTF8.GetString($buffer, 0, $read)
        $client.Close()   # the editor goes away: the request carries on
        if ($init -notmatch '<init ') { throw "not an init packet: $init" }
        if (-not $curl.WaitForExit(30000)) { throw 'the request never finished' }
        if ((Get-Content $out -Raw).Trim() -ne 'debug') { throw "the page said: $(Get-Content $out -Raw)" }
    } finally { $listener.Stop() }
}
Check 'kiln php xdebug off: the debug worker is gone, the cookie gets the plain workers' {
    Expect 'is off for PHP' (Kiln php xdebug off)
    $page = XdebugPage @('-H', 'Cookie: XDEBUG_SESSION=kiln')
    if ($page -ne 'plain') { throw "with the cookie after off: $page" }
    # The retired worker stops in the background, after Caddy has moved off it.
    $deadline = (Get-Date).AddSeconds(10)
    do {
        $debug = @(Get-CimInstance Win32_Process -Filter "Name='php-cgi.exe'" | Where-Object { $_.CommandLine -match 'xdebug' })
        if ($debug.Count) { Start-Sleep -Milliseconds 250 }
    } while ($debug.Count -and (Get-Date) -lt $deadline)
    if ($debug.Count) { throw "a debug worker is left:`n$($debug.CommandLine -join "`n")" }
}

Say '== kilnd and services (ADR 0048)'
Check 'services keep running through kiln web stop' {
    Kiln web stop | Out-Null
    if ((MailTotal) -ne 2) { throw "Mailpit isn't answering with the web server stopped" }
    Kiln web start | Out-Null
}
Check 'daemon start; services.status lists the instances through the API' {
    Kiln daemon start | Out-Null
    $out = Kiln daemon call services.status
    foreach ($name in 'mailpit-1', 'mysql-8.4', 'reverb-shop') { Expect "`"$name`"" $out }
    Expect '"running"' $out
}
Check 'service.restart through the API (the CLI does it, under its lock)' {
    Expect '"ok": true' (Kiln daemon call service.restart '{\"name\":\"mailpit-1\"}')
    if ((MailTotal) -ne 2) { throw "Mailpit has $(MailTotal) message(s) after the restart" }
    $refused = (& $Kiln daemon call service.stop '{\"name\":\"--everything\"}' 2>&1 | Out-String)
    if ($refused -notmatch 'no service called') { throw "an unknown name went through:`n$refused" }
    $global:LASTEXITCODE = 0
}

# API version 2 (ADR 0053): the CLI's --json mode, reads through it, and changes as jobs.
function JsonLines($lines) { @($lines | Where-Object { $_ } | ForEach-Object { $_ | ConvertFrom-Json }) }
function WaitJob($id) {
    $deadline = (Get-Date).AddSeconds(180)
    do {
        Start-Sleep -Milliseconds 500
        $job = (Kiln daemon call jobs.get ('{\"job\":' + $id + '}')) | ConvertFrom-Json
    } until (-not $job.running -or (Get-Date) -gt $deadline)
    if ($job.running) { throw "job $id still running after 180 s" }
    $job
}
Check 'kiln --json service list: shop.test uses mysql-8.4, read from its .env (ADR 0056)' {
    $last = (JsonLines @(& $Kiln --json service list))[-1]
    $mysql = $last.data.instances | Where-Object { $_.name -eq 'mysql-8.4' }
    if (-not ($mysql.used_by -contains 'shop.test')) { throw "mysql-8.4 used by: $($mysql.used_by -join ', ')" }
    if (-not $mysql.databases) { throw 'mysql-8.4 should hold databases' }
    $mailpit = $last.data.instances | Where-Object { $_.name -eq 'mailpit-1' }
    if ($mailpit.web -ne 'mailpit.test') { throw "mailpit-1's page: $($mailpit.web)" }
}
Check 'kiln env show: the project''s .env, secrets hidden, Kiln''s keys marked in --json' {
    $text = Kiln env show --project $shop
    Expect 'DB_PORT=' $text
    if ($text -match 'APP_KEY=base64') { throw "APP_KEY not hidden:`n$text" }
    $keys = (JsonLines @(& $Kiln --json env show --project $shop))[-1].data.keys
    $port = $keys | Where-Object { $_.key -eq 'DB_PORT' }
    if (-not $port.kiln) { throw "DB_PORT isn't marked as Kiln's: $($port | ConvertTo-Json)" }
    # The daemon reads only the folder of one of Kiln's sites, exactly as `sites` gives it.
    $project = ((JsonLines @(& $Kiln --json sites))[-1].data.sites | Where-Object { $_.host -eq 'shop.test' }).project
    $param = (ConvertTo-Json @{ project = $project } -Compress).Replace('"', '\"')
    Expect '"DB_DATABASE"' (Kiln daemon call env.show $param)
    $refused = (& $Kiln daemon call env.show '{\"project\":\"C:\\\\Windows\"}' 2>&1 | Out-String)
    if ($refused -notmatch "isn't the folder of one of Kiln's sites") { throw "a folder outside Kiln's sites was read:`n$refused" }
    $global:LASTEXITCODE = 0
}
Check 'kiln setup: what onboarding asks about, as it is on this runner (ADR 0057)' {
    $setup = (JsonLines @(& $Kiln --json setup))[-1].data
    if (-not $setup.ready) { throw "not ready: $($setup | ConvertTo-Json -Depth 4)" }
    if (-not ($setup.web.installed -and $setup.web.running)) { throw "web: $($setup.web | ConvertTo-Json)" }
    if (-not ($setup.php.versions -match '^8\.')) { throw "php: $($setup.php | ConvertTo-Json)" }
    if (-not $setup.ca.exists) { throw 'the CA was made earlier in this run' }
    if ($setup.hosts.sites -lt 1) { throw "sites: $($setup.hosts | ConvertTo-Json)" }
    Expect '"ready": true' (Kiln daemon call setup.status)
}
Check 'kiln settings: the TLD, the ports and who picked them, PHP workers (ADR 0058)' {
    $settings = (JsonLines @(& $Kiln --json settings))[-1].data
    if ($settings.tld -ne 'test') { throw "tld: $($settings.tld)" }
    if (-not ($settings.http.port -gt 0 -and $settings.https.port -gt 0)) { throw "ports: $($settings | ConvertTo-Json -Depth 3)" }
    if ($settings.php_workers -lt 1) { throw "workers: $($settings.php_workers)" }
    Expect '"php_workers"' (Kiln daemon call settings.get)
}
Check 'kiln logs: every log Kiln knows of, and following one through the daemon (ADR 0059)' {
    $sources = (JsonLines @(& $Kiln --json logs))[-1].data.sources
    foreach ($id in 'kiln', 'web', 'php') {
        if (-not ($sources | Where-Object { $_.id -eq $id })) { throw "no $id log in: $($sources.id -join ', ')" }
    }
    if (-not ($sources | Where-Object { $_.id -like 'service:*' })) { throw "no service logs in: $($sources.id -join ', ')" }
    $web = (JsonLines @(& $Kiln --json logs web --lines 5))[-1].data
    if ($web.lines.Count -lt 1) { throw "the web server's log is empty: $($web.path)" }
    $tail = (Kiln daemon call logs.tail '{\"source\":\"kiln\",\"lines\":5}') | ConvertFrom-Json
    if ($tail.offset -lt 1) { throw "logs.tail of the daemon's own log: $($tail | ConvertTo-Json)" }
    $since = (Kiln daemon call logs.tail ('{\"source\":\"kiln\",\"after\":' + $tail.offset + '}')) | ConvertFrom-Json
    if ($since.reset) { throw "a follow read started again: $($since | ConvertTo-Json)" }
    $refused = (& $Kiln daemon call logs.tail '{\"source\":\"C:\\\\Windows\\\\win.ini\"}' 2>&1 | Out-String)
    if ($refused -notmatch 'no log called') { throw "a file that isn't a log was read:`n$refused" }
    $global:LASTEXITCODE = 0
}
Check 'kiln footprint: Kiln''s processes with their memory, and its folders on disk' {
    $data = ((Kiln --json footprint) -split "`n" | Where-Object { $_ } | Select-Object -Last 1 | ConvertFrom-Json).data
    foreach ($label in 'Web server', 'PHP') {
        if (-not ($data.processes | Where-Object { $_.label -like "$label*" -and $_.memory_bytes -gt 0 })) { throw "no $label process in: $($data.processes.label -join ', ')" }
    }
    if (-not ($data.processes | Where-Object { $_.label -like 'mysql *' })) { throw "no MySQL in: $($data.processes.label -join ', ')" }
    if (-not ($data.folders | Where-Object { $_.label -eq 'PHP' -and $_.bytes -gt 10MB })) { throw "PHP's folder: $($data.folders | ConvertTo-Json)" }
    if ($data.cache_bytes -le 0) { throw 'the download cache is empty' }
    Expect '"memory_bytes"' (Kiln daemon call footprint.get)
}
Check 'kiln requests shop: the access log gives its count, median and slowest requests (ADR 0065)' {
    ShopPage
    $data = ((Kiln --json requests shop) -split "`n" | Where-Object { $_ } | Select-Object -Last 1 | ConvertFrom-Json).data
    $site = $data.sites | Where-Object { $_.host -eq 'shop.test' }
    if (-not $site -or $site.count -lt 1) { throw "no requests for shop.test: $($data | ConvertTo-Json -Depth 4)" }
    if ($site.median_ms -le 0 -or -not $site.slowest[0].uri) { throw "figures: $($site | ConvertTo-Json -Depth 4)" }
    if ($data.sites | Where-Object { $_.host -ne 'shop.test' }) { throw 'another site in a one-site answer' }
    Expect '"median_ms"' (Kiln daemon call requests.get '{\"host\":\"shop.test\"}')
    $refused = (& $Kiln daemon call requests.get '{\"host\":\"--all\"}' 2>&1 | Out-String)
    if ($refused -notmatch "isn't a site's name") { throw "a flag reached the command line:`n$refused" }
    $global:LASTEXITCODE = 0
}
function McpSession([string[]] $messages) {
    @($messages | & $Kiln mcp 2>$null | Where-Object { $_ } | ForEach-Object { $_ | ConvertFrom-Json })
}
Check 'kiln mcp: an MCP session reads sites; changes refused until allowed, then made (ADR 0066)' {
    $init = '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"ci","version":"1"}}}'
    $restart = '{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"kiln_service_restart","arguments":{"name":"mailpit-1"}}}'
    $replies = McpSession @($init, '{"jsonrpc":"2.0","method":"notifications/initialized"}',
        '{"jsonrpc":"2.0","id":2,"method":"tools/list"}',
        '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"kiln_sites","arguments":{}}}', $restart)
    if ($replies.Count -ne 4) { throw "expected 4 replies, got $($replies.Count)" }
    if ($replies[0].result.serverInfo.name -ne 'kiln') { throw "initialize: $($replies[0] | ConvertTo-Json -Depth 5)" }
    if ($replies[1].result.tools.name -contains 'kiln_service_restart') { throw 'change tools listed while changes are off' }
    if ($replies[2].result.content[0].text -notmatch 'shop\.test') { throw "kiln_sites: $($replies[2].result.content[0].text)" }
    if (-not $replies[3].result.isError) { throw 'a restart went through with changes off' }
    Kiln mcp allow-changes on | Out-Null
    try {
        $replies = McpSession @($init, $restart)
        if ($replies[1].result.isError) { throw "restart with changes allowed: $($replies[1].result.content[0].text)" }
        if ((MailTotal) -lt 0) { throw "Mailpit isn't answering after the restart" }
    } finally { Kiln mcp allow-changes off | Out-Null }
}
Check 'kiln updates: PHP patches from windows.php.net; GitHub only once turned on (ADR 0064)' {
    $last = { param($out) ($out -split "`n" | Where-Object { $_ } | Select-Object -Last 1 | ConvertFrom-Json).data }
    $data = & $last (Kiln --json updates)
    if (-not $data.php -or -not $data.php[0].newest) { throw "no PHP in: $($data | ConvertTo-Json -Depth 4)" }
    if ($data.kiln.checking) { throw 'checking GitHub is on by default' }
    Kiln updates check-kiln on | Out-Null
    $data = & $last (Kiln --json updates)
    if (-not $data.kiln.checking) { throw "check-kiln on didn't take: $($data.kiln | ConvertTo-Json)" }
    Kiln updates check-kiln off | Out-Null
    Expect '"checking": false' (Kiln daemon call updates.check)
}
Check 'kiln cache clear: the download cache is emptied; what is installed keeps working' {
    $out = (Kiln --json cache clear) -split "`n" | Where-Object { $_ } | Select-Object -Last 1 | ConvertFrom-Json
    if ($out.data.freed_bytes -le 0) { throw "nothing freed: $($out | ConvertTo-Json)" }
    $left = ((Kiln --json footprint) -split "`n" | Where-Object { $_ } | Select-Object -Last 1 | ConvertFrom-Json).data.cache_bytes
    if ($left -ne 0) { throw "$left bytes left in the cache" }
    ShopPage
}

Check 'kiln --json sites: JSON Lines ending in one result, with the sites as data' {
    $lines = @(& $Kiln --json sites)
    if ($LASTEXITCODE -ne 0) { throw "exit code $LASTEXITCODE`n$($lines -join "`n")" }
    $events = JsonLines $lines
    $last = $events[-1]
    if ($last.type -ne 'result' -or -not $last.ok) { throw "last line: $($lines[-1])" }
    if (-not ($last.data.sites | Where-Object { $_.host -eq 'shop.test' })) { throw "no shop.test in: $($lines[-1])" }
}
Check 'kiln --json undo without --confirmed: asks, stops with exit code 3, changes nothing' {
    $before = Kiln history
    $lines = @(& $Kiln --json undo)
    $code = $LASTEXITCODE
    $global:LASTEXITCODE = 0
    if ($code -ne 3) { throw "exit code $code`n$($lines -join "`n")" }
    $events = JsonLines $lines
    if (-not ($events | Where-Object { $_.type -eq 'confirm' })) { throw "no confirm event:`n$($lines -join "`n")" }
    if (-not $events[-1].needs_confirmation) { throw "last line: $($lines[-1])" }
    if ((Kiln history) -ne $before) { throw 'history changed' }
}
Check 'API v2: kiln.hello says 2; sites, services and the catalog are read through kiln --json' {
    Expect '"api": 2' (Kiln daemon call kiln.hello)
    Expect '"shop.test"' (Kiln daemon call sites.list)
    Expect '"mailpit-1"' (Kiln daemon call services.list)
    Expect '"mailpit"' (Kiln daemon call services.catalog)
    # A service's databases for its page (ADR 0068).
    Expect '"shop"' (Kiln daemon call services.databases '{\"name\":\"mysql-8.4\"}')
}
Check 'jobs: a preview stops at its confirmation, an apply runs, uninstall is refused' {
    $before = Kiln history
    $id = ((Kiln daemon call jobs.start '{\"args\":[\"undo\"],\"mode\":\"preview\"}') | ConvertFrom-Json).job
    $job = WaitJob $id
    if (-not $job.finished.needs_confirmation) { throw "the preview didn't stop at its confirmation: $($job | ConvertTo-Json -Depth 6)" }
    if (-not ($job.events | Where-Object { $_.type -eq 'confirm' })) { throw "no confirm event: $($job | ConvertTo-Json -Depth 6)" }
    if ((Kiln history) -ne $before) { throw 'a preview changed the history' }
    $id = ((Kiln daemon call jobs.start '{\"args\":[\"service\",\"restart\",\"mailpit-1\"],\"mode\":\"apply\"}') | ConvertFrom-Json).job
    $job = WaitJob $id
    if (-not $job.finished.ok) { throw "the restart job failed: $($job | ConvertTo-Json -Depth 6)" }
    if ((MailTotal) -ne 2) { throw "Mailpit has $(MailTotal) message(s) after the job" }
    $refused = (& $Kiln daemon call jobs.start '{\"args\":[\"uninstall\"],\"mode\":\"apply\"}' 2>&1 | Out-String)
    if ($refused -notmatch "can't be run through the API") { throw "uninstall wasn't refused:`n$refused" }
    $refused = (& $Kiln daemon call jobs.start '{\"args\":[\"php\",\"install\",\"8.4\"],\"mode\":\"preview\"}' 2>&1 | Out-String)
    if ($refused -notmatch 'without asking') { throw "a preview of a change that doesn't ask went through:`n$refused" }
    $global:LASTEXITCODE = 0
}
Check 'kiln app enable: the login item starts the app in the tray; undo puts back what was there (ADR 0054 §6)' {
    # This job doesn't build the app: for the login item, a stand-in file next to kiln is enough.
    $app = Join-Path (Split-Path $Kiln) 'kiln-desktop.exe'
    $standIn = -not (Test-Path $app)
    if ($standIn) { Copy-Item (Join-Path (Split-Path $Kiln) 'kilnd.exe') $app }
    $run = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
    try {
        $before = (Get-ItemProperty $run -Name Kiln -ErrorAction SilentlyContinue).Kiln
        Kiln app enable --yes | Out-Null
        $now = (Get-ItemProperty $run -Name Kiln -ErrorAction SilentlyContinue).Kiln
        if ($now -ne "`"$app`" --background") { throw "the Run value is: $now" }
        Expect "Kiln's app" (Kiln daemon status)
        Expect "Kiln's app (in the tray) starts at login" (Kiln history)
        Kiln undo --yes | Out-Null
        $after = (Get-ItemProperty $run -Name Kiln -ErrorAction SilentlyContinue).Kiln
        if ($after -ne $before) { throw "undo left '$after', not '$before'" }
    } finally {
        if ($standIn) { Remove-Item $app -ErrorAction SilentlyContinue }
    }
}
Check 'the services supervisor crashes: the daemon brings every service back' {
    # Kiln's own record of it: the first line of its PID file.
    $pidFile = "$env:LOCALAPPDATA\Kiln\services\supervisor\supervisor.pid"
    $supervisor = [int]((Get-Content $pidFile -TotalCount 1) -as [int])
    if (-not $supervisor -or -not (Get-Process -Id $supervisor -ErrorAction SilentlyContinue)) {
        throw "no services supervisor running (PID file: $(Get-Content $pidFile -Raw -ErrorAction SilentlyContinue))"
    }
    # Which process serves Mailpit before the crash: it must be gone after it (I-96).
    $owner = (Get-NetTCPConnection -LocalPort $script:http -State Listen -ErrorAction SilentlyContinue | Select-Object -First 1).OwningProcess
    Stop-Process -Id $supervisor -Force
    $deadline = (Get-Date).AddSeconds(5)
    while ($owner -and (Get-Process -Id $owner -ErrorAction SilentlyContinue) -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 200 }
    if ($owner -and (Get-Process -Id $owner -ErrorAction SilentlyContinue)) {
        $p = Get-CimInstance Win32_Process -Filter "ProcessId=$owner"
        Say "     Mailpit (PID $owner, parent $($p.ParentProcessId)) outlived its supervisor: $($p.CommandLine)"
        Get-Content "$env:LOCALAPPDATA\Kiln\services\supervisor\supervisor.log" -Tail 15 -ErrorAction SilentlyContinue | ForEach-Object { Say "     supervisor: $_" }
        Get-Content "$env:LOCALAPPDATA\Kiln\logs\kilnd.log" -Tail 15 -ErrorAction SilentlyContinue | ForEach-Object { Say "     kilnd: $_" }
        throw 'Mailpit still runs: the crash took nothing down'
    }
    $deadline = (Get-Date).AddSeconds(90)
    do {
        Start-Sleep -Seconds 3
        $back = Test-NetConnection 127.0.0.1 -Port $script:http -InformationLevel Quiet -WarningAction SilentlyContinue
    } until ($back -or (Get-Date) -gt $deadline)
    # What happened, in the scenario's log whichever way it went.
    Say "     killed supervisor PID $supervisor; now: $(Get-Content $pidFile -TotalCount 1 -ErrorAction SilentlyContinue)"
    Get-Content "$env:LOCALAPPDATA\Kiln\logs\kilnd.log" -Tail 12 -ErrorAction SilentlyContinue | ForEach-Object { Say "     kilnd: $_" }
    Get-Content "$env:LOCALAPPDATA\Kiln\services\supervisor\supervisor.log" -Tail 8 -ErrorAction SilentlyContinue | ForEach-Object { Say "     supervisor: $_" }
    if (-not $back) { throw 'not back after 90 s' }
    if ((MailTotal) -ne 2) { throw "Mailpit is back with $(MailTotal) message(s)" }
    # The restart is counted once `kiln services resume` returns, after the slower databases
    # are ready too, which is after Mailpit is back.
    $deadline = (Get-Date).AddSeconds(120)
    do {
        Start-Sleep -Seconds 3
        $status = Kiln daemon status
    } until ($status -match 'Services restarted by the daemon: 1' -or (Get-Date) -gt $deadline)
    if ($status -notmatch 'Services restarted by the daemon: 1') {
        Get-Content "$env:LOCALAPPDATA\Kiln\logs\kilnd.log" -Tail 20 -ErrorAction SilentlyContinue | ForEach-Object { Say "     kilnd: $_" }
        Get-Content "$env:LOCALAPPDATA\Kiln\services\supervisor\supervisor.log" -Tail 10 -ErrorAction SilentlyContinue | ForEach-Object { Say "     supervisor: $_" }
    }
    Expect 'Services restarted by the daemon: 1' $status
    Expect 'Services: 12 of 12 running' $status
}
# A reboot, as far as a runner can have one: every process of Kiln's gone, the services'
# supervisor leaving its PID file behind, then the daemon starting as it does at login.
Check 'after a reboot (daemon and supervisor gone, daemon started as at login): services come back' {
    Kiln daemon stop | Out-Null
    $pidFile = "$env:LOCALAPPDATA\Kiln\services\supervisor\supervisor.pid"
    Stop-Process -Id ([int](Get-Content $pidFile -TotalCount 1)) -Force
    Start-Sleep -Seconds 2
    if (Test-NetConnection 127.0.0.1 -Port $script:http -InformationLevel Quiet -WarningAction SilentlyContinue) {
        throw 'Mailpit still answers'
    }
    Kiln daemon start | Out-Null
    $deadline = (Get-Date).AddSeconds(150)
    do {
        Start-Sleep -Seconds 3
        $status = Kiln daemon status
    } until ($status -match 'Services restarted by the daemon: 1' -or (Get-Date) -gt $deadline)
    if ($status -notmatch 'Services restarted by the daemon: 1') {
        Get-Content "$env:LOCALAPPDATA\Kiln\logs\kilnd.log" -Tail 20 -ErrorAction SilentlyContinue | ForEach-Object { Say "     kilnd: $_" }
        Get-Content "$env:LOCALAPPDATA\Kiln\services\supervisor\supervisor.log" -Tail 10 -ErrorAction SilentlyContinue | ForEach-Object { Say "     supervisor: $_" }
    }
    Expect 'Services restarted by the daemon: 1' $status
    Expect 'Services: 12 of 12 running' $status
    if ((MailTotal) -ne 2) { throw "Mailpit is back with $(MailTotal) message(s)" }
}
Check 'daemon stop' { Kiln daemon stop | Out-Null }

Say '== Data safety: dump and restore (ADR 0049)'
$dumps = Join-Path $dir 'dumps'
New-Item -ItemType Directory -Force $dumps | Out-Null
Check 'kiln db dump mysql-8.4 shop: the Laravel app''s database as a portable .sql file' {
    Expect 'Wrote' (Kiln db dump mysql-8.4 shop --output "$dumps\shop.sql")
    Expect 'CREATE TABLE `migrations`' (Get-Content "$dumps\shop.sql" -Raw)
}
Check 'an existing dump is never overwritten' {
    $out = (& $Kiln db dump mysql-8.4 shop --output "$dumps\shop.sql" 2>&1 | Out-String)
    if ($LASTEXITCODE -eq 0 -or $out -notmatch 'cannot write') { throw "it wrote over the file:`n$out" }
    $global:LASTEXITCODE = 0
}
Check 'kiln db restore mysql-8.4 shop.sql shop_copy: a new database with the same rows' {
    Expect 'Created the database shop_copy' (Kiln db restore mysql-8.4 "$dumps\shop.sql" shop_copy)
    $original = Client 'mysql-8.4' 'shop' 'SELECT COUNT(*) FROM migrations;'
    $copy = Client 'mysql-8.4' 'shop_copy' 'SELECT COUNT(*) FROM migrations;'
    if ($copy -ne $original -or $copy -notmatch '[1-9]') { throw "original:`n$original`ncopy:`n$copy" }
}
Check 'PostgreSQL: pg_dump kiln_ci, restored as kiln_copy' {
    Kiln db dump postgres-18 kiln_ci --output "$dumps\kiln_ci.sql" | Out-Null
    Kiln db restore postgres-18 "$dumps\kiln_ci.sql" kiln_copy | Out-Null
    Expect 'kept' (Client 'postgres-18' 'kiln_copy' 'SELECT v FROM t;')
}
Check 'kiln db dump postgres-18: every database (pg_dumpall)' {
    Kiln db dump postgres-18 --output "$dumps\postgres-all.sql" | Out-Null
    Expect 'kiln_copy' (Get-Content "$dumps\postgres-all.sql" -Raw)
}
Say '== A service''s databases (ADR 0068)'
Check 'kiln db create, list and drop: made, listed with its size, kept when the drop is declined, then gone' {
    Expect 'Created the database kiln_scratch' (Kiln db create mysql-8.4 kiln_scratch)
    $list = JsonLines @(& $Kiln --json db list mysql-8.4)
    $names = $list[-1].data.databases.name
    if ($names -notcontains 'kiln_scratch' -or $names -notcontains 'shop' -or $names -contains 'mysql') { throw "databases: $($names -join ', ')" }
    $declined = @(& $Kiln --json db drop mysql-8.4 kiln_scratch)
    $code = $LASTEXITCODE
    $global:LASTEXITCODE = 0
    if ($code -ne 3) { throw "a drop without --confirmed ended with $code`n$($declined -join "`n")" }
    if ((Client 'mysql-8.4' 'kiln_scratch' 'SELECT 1;') -notmatch '1') { throw 'the declined drop deleted it' }
    Expect 'Deleted the database kiln_scratch' (Kiln --json --confirmed db drop mysql-8.4 kiln_scratch)
    $after = (JsonLines @(& $Kiln --json db list mysql-8.4))[-1].data.databases.name
    if ($after -contains 'kiln_scratch') { throw 'still listed after the drop' }
    $refused = (& $Kiln db create mysql-8.4 'a`;b' 2>&1 | Out-String)
    $global:LASTEXITCODE = 0
    Expect "can't be a database name" $refused
}
Check 'a service without a dump tool says where its data is and what keeps it' {
    $out = (& $Kiln db dump redis-8.10 2>&1 | Out-String)
    Expect 'has no dump tool' $out
    Expect '--keep-data' $out
    $global:LASTEXITCODE = 0
}

Check 'servers.json follows the services: instances added after the tools are listed' {
    $servers = Get-Content "$env:LOCALAPPDATA\Kiln\tools\servers.json" -Raw | ConvertFrom-Json
    foreach ($name in 'mysql-8.4', 'redis-8.10', 'mongodb-9') {
        if ($servers.name -notcontains $name) { throw "$name missing from:`n$($servers | ConvertTo-Json -Compress)" }
    }
}
Check 'tool remove adminer: the site and its files go' {
    Kiln tool remove adminer | Out-Null
    if (Test-Path "$env:LOCALAPPDATA\Kiln\tools\adminer") { throw 'its folder is still there' }
    if ((Kiln sites) -match 'adminer\.test') { throw 'still a site' }
}

Say '== uninstall'
Check 'kiln uninstall refuses while services hold data, and changes nothing' {
    $out = (& $Kiln uninstall --yes 2>&1 | ForEach-Object { "$_" }) -join "`n"
    if ($LASTEXITCODE -eq 0) { throw "uninstall went ahead without --delete-data:`n$out" }
    Expect 'hold data' $out
    Expect 'kiln uninstall --keep-data' $out
    if (-not (Test-Path "$env:LOCALAPPDATA\Kiln")) { throw 'Kiln folders were touched' }
    $global:LASTEXITCODE = 0
}
Check '--keep-data and --delete-data can''t be given together' {
    $out = (& $Kiln uninstall --yes --keep-data --delete-data 2>&1 | Out-String)
    if ($LASTEXITCODE -eq 0) { throw "both were taken:`n$out" }
    $global:LASTEXITCODE = 0
}
$script:kept = ''
# The workflow runs the scenario once per uninstall mode (P3-12): both must leave the machine
# as it was, apart from the kept data folder.
if ($Uninstall -eq 'delete-data') {
    Check 'kiln uninstall --delete-data: the services'' data deleted with Kiln' {
        $out = Kiln uninstall --yes --delete-data
        $out -split "`n" | ForEach-Object { Say "     $_" }
        if ($out -match 'Copied the services') { throw 'it kept data without --keep-data' }
    }
} else { Check 'kiln uninstall --keep-data: the services'' data copied to the home folder first' {
    $out = Kiln uninstall --yes --keep-data
    $out -split "`n" | ForEach-Object { Say "     $_" }
    $script:kept = [regex]::Match($out, 'Copied the services'' data to (.+)\.').Groups[1].Value.Trim()
    if (-not $script:kept -or -not (Test-Path $script:kept)) { throw "no kept folder in:`n$out" }
    foreach ($instance in 'mysql-8.4', 'postgres-18', 'mailpit-1', 'redis-8.10') {
        if (-not (Get-ChildItem (Join-Path $script:kept $instance) -Recurse -File -ErrorAction SilentlyContinue)) {
            throw "$instance's data isn't in $($script:kept)"
        }
    }
    if (-not (Get-ChildItem (Join-Path $script:kept 'mysql-8.4\shop') -ErrorAction SilentlyContinue)) {
        throw 'the shop database isn''t in the kept MySQL data'
    }
} }
# The kept data is the one thing uninstall leaves; the test removes it before comparing.
if ($script:kept) { Remove-Item $script:kept -Recurse -Force -ErrorAction SilentlyContinue }
Check 'uninstall left the project''s .env as it was: still pointed at MySQL' {
    if ((Get-Content $envFile -Raw) -notmatch '(?m)^DB_CONNECTION=mysql') { throw 'the .env was changed' }
}
Start-Sleep -Seconds 2
& (Join-Path $vm 'snapshot.ps1') | Set-Content (Join-Path $dir 'after.txt') -Encoding utf8
Compare-Snapshot 'after.txt' 'after uninstall'
if (Test-Path "$env:LOCALAPPDATA\Kiln") {
    Get-ChildItem "$env:LOCALAPPDATA\Kiln" -Recurse -Force | Select-Object -First 20 | ForEach-Object { Say "     left: $($_.FullName)" }
}

Say $(if ($script:failed) { '== FAILED' } else { '== PASSED' })
exit $(if ($script:failed) { 1 } else { 0 })
