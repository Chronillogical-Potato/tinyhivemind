//! Public research sources mirrored into the shared workspace before turns start.

use std::path::Path;
use std::time::Duration;

pub(super) async fn stage_research_sources(scratch: &Path) -> anyhow::Result<()> {
    let directory = scratch.join("research_sources");
    std::fs::create_dir_all(&directory)?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()?;
    let sources = [
        (
            "rauzy.md",
            "https://raw.githubusercontent.com/senamakel/math-superagent/f0b35053424007d21d71363ce4ed73e0c8baca9e/workspace/project-euler/1006/research/approaches/pe1006-rauzy-block-semidir-product.md",
        ),
        (
            "approaches.md",
            "https://raw.githubusercontent.com/senamakel/math-superagent/f0b35053424007d21d71363ce4ed73e0c8baca9e/workspace/project-euler/1006/derived/APPROACHES.md",
        ),
        (
            "verification.md",
            "https://raw.githubusercontent.com/senamakel/math-superagent/f0b35053424007d21d71363ce4ed73e0c8baca9e/workspace/project-euler/1006/code/out/PE1006-verification.md",
        ),
        (
            "external_approach.md",
            "https://raw.githubusercontent.com/dawei7/code_n/012e178619373894a06afb8db07953df0202a071/dsa/euler/1006_fibonacci-subwords/variants/optimal/approach.md",
        ),
        (
            "external_solution.py",
            "https://raw.githubusercontent.com/dawei7/code_n/012e178619373894a06afb8db07953df0202a071/dsa/euler/1006_fibonacci-subwords/variants/optimal/solutions/solution.py",
        ),
        (
            "external_cases.json",
            "https://raw.githubusercontent.com/dawei7/code_n/012e178619373894a06afb8db07953df0202a071/dsa/euler/1006_fibonacci-subwords/cases.json",
        ),
        (
            "eulersolve_solution.py",
            "https://eulersolve.org/solutionsPython/Euler1006.py",
        ),
        (
            "eulersolve_explanation.html",
            "https://eulersolve.org/problem/1006/",
        ),
        (
            "cirosantilli_1006.md",
            "https://raw.githubusercontent.com/cirosantilli/project-euler-solutions/master/solvers/1006.md",
        ),
    ];
    for (name, url) in sources {
        let target = directory.join(name);
        if target.is_file() {
            println!("using cached public research source: {name}");
            continue;
        }
        let body = match client
            .get(url)
            .send()
            .await
            .and_then(|reply| reply.error_for_status())
        {
            Ok(reply) => reply.text().await?,
            Err(error) if name != "eulersolve_solution.py" => {
                println!("optional research source unavailable: {name} ({error})");
                continue;
            }
            Err(error) => return Err(error.into()),
        };
        let source = if name.ends_with(".py") {
            format!("# Source: {url}\n\n{body}")
        } else {
            format!("Source: {url}\n\n{body}")
        };
        std::fs::write(target, source)?;
    }
    stage_authenticated_github_source(
        &directory,
        "candidate_euler1006.py",
        "repos/senamakel/math-agent/contents/workspace/euler1006/code/lean/code/python/euler1006.py?ref=be919bc1bdc6b77a075413192654931b80cae602",
        "https://github.com/senamakel/math-agent/blob/be919bc1bdc6b77a075413192654931b80cae602/workspace/euler1006/code/lean/code/python/euler1006.py",
    )?;
    stage_authenticated_github_source(
        &directory,
        "candidate_context.md",
        "repos/senamakel/math-agent/contents/workspace/euler1006/refs/context.md?ref=be919bc1bdc6b77a075413192654931b80cae602",
        "https://github.com/senamakel/math-agent/blob/be919bc1bdc6b77a075413192654931b80cae602/workspace/euler1006/refs/context.md",
    )?;
    Ok(())
}

fn stage_authenticated_github_source(
    directory: &Path,
    name: &str,
    endpoint: &str,
    source_url: &str,
) -> anyhow::Result<()> {
    let target = directory.join(name);
    if target.is_file() {
        println!("using cached authenticated research source: {name}");
        return Ok(());
    }
    let output = std::process::Command::new("gh")
        .args([
            "api",
            "-H",
            "Accept: application/vnd.github.raw+json",
            endpoint,
        ])
        .output()?;
    if !output.status.success() {
        anyhow::bail!("gh api could not stage {name}");
    }
    let mut body = format!("Source: {source_url}\n\n").into_bytes();
    body.extend(output.stdout);
    std::fs::write(target, body)?;
    Ok(())
}
