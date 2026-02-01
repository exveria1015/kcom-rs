$ErrorActionPreference = "Stop"

function Run-Test {
    param(
        [Parameter(Mandatory = $true)][string]$Name,
        [Parameter(Mandatory = $false)][string[]]$Args = @()
    )

    Write-Host ""
    Write-Host "==> $Name"
    if ($Args.Count -gt 0) {
        Write-Host "cargo test $($Args -join ' ')"
    } else {
        Write-Host "cargo test"
    }

    & cargo test @Args
}

Run-Test -Name "default"
Run-Test -Name "async-com" -Args @("--features", "async-com")
Run-Test -Name "kernel-unicode" -Args @("--features", "kernel-unicode")
Run-Test -Name "refcount-hardening" -Args @("--features", "refcount-hardening")
Run-Test -Name "combo" -Args @("--features", "async-com kernel-unicode refcount-hardening")
