param(
    [int]$IntervalSeconds = 60,
    [int]$MaxChecks = 120
)

$repo = (Get-Location).Path
$log = Join-Path $repo "ci-watch.log"
$seen = @{}
$finished = $false

function Write-Log([string]$msg) {
    $line = "[{0}] {1}" -f (Get-Date -Format "yyyy-MM-dd HH:mm:ss"), $msg
    Add-Content -Path $log -Value $line
    Write-Host $line
}

Write-Log "CI watch started (interval=$IntervalSeconds s, max=$MaxChecks checks)"

for ($i = 0; $i -lt $MaxChecks; $i++) {
    $raw = & gh run list --workflow=package.yml --limit 10 2>&1 | Out-String
    $lines = $raw -split "`r?`n" | Where-Object { $_.Trim() -ne "" }

    foreach ($line in $lines) {
        # gh run list columns: STATUS CONCLUSION TITLE WORKFLOW BRANCH EVENT ID DURATION TIME
        $cols = $line -split "`t" | Where-Object { $_ -ne "" }
        if ($cols.Count -lt 7) { continue }
        $status = $cols[0].Trim()
        $conclusion = $cols[1].Trim()
        $id = $cols[6].Trim()
        $key = "$id-$status-$conclusion"
        if ($seen.ContainsKey($key)) { continue }
        $seen[$key] = $true

        if ($status -eq "completed") {
            Write-Log "RUN $id => $conclusion : $line"
            if ($conclusion -eq "failure") {
                $failed = & gh run view $id --log-failed 2>&1 | Out-String
                Add-Content -Path $log -Value "----- FAILED LOG run $id -----"
                Add-Content -Path $log -Value $failed
                Add-Content -Path $log -Value "----- END LOG run $id -----"
                Write-Log "Captured failed log for run $id"
            }
        }
    }

    # Stop when both latest runs (main push + tag push) reached a conclusion
    $mainDone = $false; $tagDone = $false
    foreach ($line in $lines) {
        if ($line -match "in_progress|queued") { continue }
        if ($line -match "`tmain`tpush`t") { $mainDone = $true }
        if ($line -match "`t3\.1\.1`tpush`t") { $tagDone = $true }
    }
    if ($mainDone -and $tagDone) {
        Write-Log "Both main and tag runs concluded. Watcher stopping."
        $finished = $true
        break
    }

    Start-Sleep -Seconds $IntervalSeconds
}

if (-not $finished) { Write-Log "Watcher reached max checks without both runs concluding." }
Write-Log "CI watch ended."
