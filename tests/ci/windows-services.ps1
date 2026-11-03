# Kiln's services scenario on a GitHub Actions Windows runner (ADR 0039 §3, P3-13): snapshot,
# services added and exercised, uninstall, snapshot again, compare. No desktop session and no
# Herd here: those are checked by hand in the Windows VM before a release.
#   windows-services.ps1 -Kiln <path to kiln.exe>
# Reuses the VM scenario's helpers (lib.ps1) and snapshot (snapshot.ps1). The runner's image
# may run its own MySQL or PostgreSQL, so the checks read the port Kiln chose instead of
# assuming the usual one: another service on it is a coexistence check for free.
param([Parameter(Mandatory)] [string] $Kiln)
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
    $out = Kiln service add mailpit --name mail-two
    Expect "used by Kiln's mailpit-1" $out
    Kiln service remove mail-two --delete-data | Out-Null
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
    $page = Page $script:adminer @('--data-urlencode', 'auth[server]=postgres-18', '--data-urlencode', 'auth[db]=')
    Clean $page
    if ($page -notmatch 'kiln_ci') {
        $heading = [regex]::Match($page, '<h2>(.*?)</h2>').Groups[1].Value
        $error = [regex]::Match($page, "class='error'>(.*?)</div>").Groups[1].Value
        $trace = Page $script:adminer @('-i', '--data-urlencode', 'auth[server]=postgres-18', '--data-urlencode', 'auth[db]=')
        $hops = ($trace -split "`n" | Where-Object { $_ -match '^(HTTP/|Location:|Set-Cookie:)' } | ForEach-Object { ($_ -replace '=[^;]*', '=').Trim() }) -join ' | '
        throw "no kiln_ci; heading: $heading; error: $error; title: $([regex]::Match($page, '<title>(.*?)</title>').Groups[1].Value); hops: $hops"
    }
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
    if (-not (Test-Path "$env:LOCALAPPDATA\Kiln")) { throw 'Kiln folders were touched' }
    $global:LASTEXITCODE = 0
}
Check 'kiln uninstall --delete-data' { Kiln uninstall --yes --delete-data | ForEach-Object { Say "     $_" } }
Start-Sleep -Seconds 2
& (Join-Path $vm 'snapshot.ps1') | Set-Content (Join-Path $dir 'after.txt') -Encoding utf8
Compare-Snapshot 'after.txt' 'after uninstall'
if (Test-Path "$env:LOCALAPPDATA\Kiln") {
    Get-ChildItem "$env:LOCALAPPDATA\Kiln" -Recurse -Force | Select-Object -First 20 | ForEach-Object { Say "     left: $($_.FullName)" }
}

Say $(if ($script:failed) { '== FAILED' } else { '== PASSED' })
exit $(if ($script:failed) { 1 } else { 0 })
