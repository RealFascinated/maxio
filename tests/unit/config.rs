use clap::Parser;
use maxio::config::{Config, PoolSettings};

#[derive(Parser, Debug)]
struct TestCli {
    #[command(flatten)]
    config: Config,
}

#[test]
fn default_address_is_all_interfaces() {
    unsafe {
        std::env::remove_var("MAXIO_ADDRESS");
    }

    let cli = TestCli::parse_from(["maxio", "--database-url", "postgres://localhost/maxio"]);

    assert_eq!(cli.config.address, "0.0.0.0");
}

#[test]
fn default_db_pool_settings() {
    let cli = TestCli::parse_from(["maxio", "--database-url", "postgres://localhost/maxio"]);

    assert_eq!(cli.config.db_pool_size, 64);
    assert!(cli.config.db_prepared_statement_cache);
    assert_eq!(PoolSettings::from(&cli.config).max_size, 64);
}
