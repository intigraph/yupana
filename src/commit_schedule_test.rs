use super::*;

fn count(path: &str, text: &str) -> usize {
    facts(path, text.as_bytes()).unwrap().len()
}

#[test]
fn timer_values_resets_comments_and_shrink_only() {
    let old = facts("job.timer", b"[Timer]\nOnCalendar=hourly\n").unwrap();
    assert_eq!(old.len(), 1);
    assert!(introduced(old.clone(), old.clone()).is_empty());
    assert!(introduced(old.clone(), Vec::new()).is_empty());
    let new = facts("job.timer", b"[Timer]\nOnCalendar=daily\n").unwrap();
    assert_eq!(introduced(old, new).len(), 1);
    assert_eq!(
        count(
            "job.timer",
            "# OnCalendar=hourly\n[Unit]\nDescription=OnBootSec=5\n"
        ),
        0
    );
    assert_eq!(count("job.timer", "[Timer]\nOnCalendar=\n"), 0);
    assert_eq!(count("job.timer", "[Timer]\nOnUnitActiveSec=5m\n"), 1);
    assert_eq!(count("job.timer", "[Timer]\nOnBootSec=30\n"), 1);
    let old = facts(
        "job.timer",
        b"[Timer]\nOnCalendar=hourly\nOnCalendar=daily\n",
    )
    .unwrap();
    let new = facts("job.timer", b"[Timer]\nOnCalendar=hourly\n").unwrap();
    assert!(introduced(old, new).is_empty());
    let reset = facts("job.timer", b"[Timer]\nOnCalendar=hourly\nOnCalendar=\n").unwrap();
    assert!(reset.is_empty());
    assert_eq!(
        introduced(
            reset,
            facts("job.timer", b"[Timer]\nOnCalendar=hourly\n").unwrap()
        )
        .len(),
        1
    );
}

#[test]
fn duplicate_new_cron_entries_are_additions() {
    let old = facts("cron.d/job", b"0 * * * * root task\n").unwrap();
    let new = facts("cron.d/job", b"0 * * * * root task\n0 * * * * root task\n").unwrap();
    assert_eq!(introduced(old, new).len(), 1);
}

#[test]
fn plist_is_parsed_including_entities_and_values() {
    let a = b"<plist><dict><key>StartInterval</key><integer>60</integer></dict></plist>";
    let b = b"<plist><dict><key>StartInterval</key><integer>30</integer></dict></plist>";
    let old = facts("job.plist", a).unwrap();
    assert_eq!(old.len(), 1);
    assert_eq!(introduced(old, facts("job.plist", b).unwrap()).len(), 1);
    assert_eq!(
        count(
            "job.plist",
            "<plist><dict><!-- <key>StartInterval</key> --></dict></plist>"
        ),
        0
    );
    assert!(facts("job.plist", b"<plist><dict>").is_err());
    let old = facts("job.plist", b"<plist><dict><key>StartCalendarInterval</key><array><dict><key>Hour</key><integer>1</integer></dict><dict><key>Hour</key><integer>2</integer></dict></array></dict></plist>").unwrap();
    let new = facts("job.plist", b"<plist><dict><key>StartCalendarInterval</key><array><dict><key>Hour</key><integer>1</integer></dict></array></dict></plist>").unwrap();
    assert!(introduced(old, new).is_empty());
}

#[test]
fn workflow_exempts_only_ci_trigger_not_commands() {
    let trigger = "on:\n  schedule:\n    - cron: '0 * * * *'\njobs: {}\n";
    assert_eq!(count(".github/workflows/ci.yml", trigger), 0);
    assert_eq!(count(".forgejo/workflows/ci.yml", trigger), 0);
    assert_eq!(count("config.yml", trigger), 1);
    assert_eq!(
        count(
            ".github/workflows/ci.yml",
            "jobs:\n  deploy:\n    steps:\n      - run: crontab -\n"
        ),
        1
    );
}

#[test]
fn ansible_flow_nested_alias_and_removal() {
    assert_eq!(
        count(
            "tasks.yml",
            "- ansible.builtin.cron: {name: job, minute: '0', job: task}\n"
        ),
        1
    );
    assert_eq!(
        count(
            "tasks.yml",
            "- community.general.cron: {name: job, state: absent}\n"
        ),
        0
    );
    let alias = "base: &clock\n  minute: '0'\n  job: task\ntasks:\n  - cron: *clock\n";
    assert_eq!(count("tasks.yml", alias), 1);
    assert!(facts("tasks.yml", b"- cron: [unterminated").is_err());
    let a = facts(
        "tasks.yml",
        b"- cron: {name: job, minute: '0', job: task}\n",
    )
    .unwrap();
    let b = facts(
        "tasks.yml",
        b"- cron: {job: task, minute: '0', name: job}\n",
    )
    .unwrap();
    assert!(introduced(a, b).is_empty());
    let old = facts("config.yml", b"schedule: [hourly, daily]\n").unwrap();
    let new = facts("config.yml", b"schedule: [hourly]\n").unwrap();
    assert!(introduced(old, new).is_empty());
}

#[test]
fn shell_actual_commands_not_comments_printed_text_or_heredocs() {
    for code in [
        "crontab -",
        "crontab jobs",
        "sudo crontab -",
        "env crontab -",
        "printf x | crontab -",
        "bash -c 'crontab jobs'",
        "echo x > /etc/cron.d/job",
    ] {
        assert!(count("task.sh", code) > 0, "missed {code}");
    }
    for code in [
        "crontab -l",
        "crontab --help",
        "echo 'crontab -'",
        "# crontab -",
        "cat <<'DOC'\ncrontab -\nDOC\n",
        "cat /etc/cron.d/job",
    ] {
        assert_eq!(count("task.sh", code), 0, "false positive {code}");
    }
    assert!(facts("task.sh", b"crontab '\n").is_err());
}

#[test]
fn docs_are_not_installed_scheduler_units() {
    assert_eq!(
        count("README.md", "```sh\ncrontab -\n```\nOnCalendar=hourly"),
        0
    );
}

#[test]
fn structured_data_tokens_are_not_scheduler_source() {
    let bytes = b"crontab -\nOnCalendar=hourly\nStartInterval\n/etc/cron.d/job\n";
    for extension in ["json", "jsonl", "nt", "nq", "ttl", "csv"] {
        assert!(
            facts(&format!("archive.{extension}"), bytes)
                .unwrap()
                .is_empty(),
            "{extension}"
        );
    }
    // The exception is format-bound. Executable configuration still yields a
    // scheduler fact and unsupported executable source remains explicitly unknown.
    assert!(!facts("clock.timer", b"[Timer]\nOnCalendar=hourly\n")
        .unwrap()
        .is_empty());
    assert!(facts(
        "task.py",
        b"import subprocess\nsubprocess.run(['crontab', '-'])\n"
    )
    .is_err());
}
