use anyhow::Result;
use cliclack::{input, intro, outro, select};
use console::style;
use goose::config::Config;
use goose::model::ModelConfig;

pub async fn handle_zai_configure() -> Result<()> {
    let config = Config::global();

    intro(style(" 🚀 Z.ai Coding Agent ").on_cyan().black())?;

    // Check if ZAI_API_KEY is already set
    let api_key = if let Ok(key) = std::env::var("ZAI_API_KEY") {
        println!(
            "\n  {} Z.ai API key found in environment variables",
            style("✓").green()
        );
        key
    } else {
        // Prompt for API key
        println!(
            "\n  {} Get your API key at: {}",
            style("ℹ").blue(),
            style("https://z.ai/subscribe").cyan()
        );

        let key: String = input("Enter your Z.ai API key:")
            .validate(|input: &String| {
                if input.trim().is_empty() {
                    Err("API key cannot be empty")
                } else {
                    Ok(())
                }
            })
            .interact()?;

        // Save the API key
        config.set_secret("ZAI_API_KEY", &key)?;
        println!("  {} API key saved", style("✓").green());
        key
    };

    // Select model
    println!("\n  {} Select your preferred model:", style("ℹ").blue());
    let model_choice: &str = select("Choose a model:")
        .item("glm-4.6", "GLM-4.6", "200K context, best for coding")
        .item("glm-4.5", "GLM-4.5", "128K context, stable")
        .item("glm-4.5-air", "GLM-4.5-Air", "Fast and economical")
        .interact()?;

    // Save the model choice
    config.set_param("GOOSE_MODEL", model_choice)?;
    println!("  {} Model set to: {}", style("✓").green(), model_choice);

    // Test the configuration
    println!("\n  {} Testing configuration...", style("ℹ").blue());
    match test_zai_config(&api_key, model_choice).await {
        Ok(_) => {
            println!("  {} Configuration test successful!", style("✓").green());
        }
        Err(e) => {
            println!("  {} Configuration test failed: {}", style("✗").red(), e);
            println!(
                "  {} Please check your API key and try again",
                style("ℹ").blue()
            );
            return Err(e);
        }
    }

    outro("✓ Configuration completed! You can now use 'zai' to start coding with AI.")?;
    Ok(())
}

pub async fn handle_config_show() -> Result<()> {
    let config = Config::global();

    println!("\n{} Z.ai Configuration", style("🔧").blue());
    println!("{}", "─".repeat(30));

    // Show API key status (masked)
    match config.get_secret::<String>("ZAI_API_KEY") {
        Ok(key) => {
            let masked = if key.len() > 8 {
                format!("sk-{}***", &key[3..8])
            } else {
                "sk-***".to_string()
            };
            println!("API Key: {}", masked);
        }
        Err(_) => {
            println!("API Key: {}", style("Not set").red());
        }
    }

    // Show current model
    match config.get_param::<String>("GOOSE_MODEL") {
        Ok(model) => {
            println!("Model: {}", model);
        }
        Err(_) => {
            println!("Model: {} (default)", style("glm-4.6").dim());
        }
    }

    println!();
    Ok(())
}

pub async fn handle_config_model(model: String) -> Result<()> {
    let config = Config::global();

    config.set_param("GOOSE_MODEL", &model)?;
    println!("Model updated to: {}", style(&model).green());

    // Test the new model
    println!("Testing configuration...");
    match config.get_secret::<String>("ZAI_API_KEY") {
        Ok(api_key) => match test_zai_config(&api_key, &model).await {
            Ok(_) => {
                println!("{} Model test successful!", style("✓").green());
            }
            Err(e) => {
                println!("{} Model test failed: {}", style("✗").red(), e);
                return Err(e);
            }
        },
        Err(_) => {
            println!(
                "{} API key not set. Please run 'zai config' to set up your API key.",
                style("⚠").yellow()
            );
        }
    }

    Ok(())
}

pub async fn handle_config_key() -> Result<()> {
    let config = Config::global();

    println!(
        "\n  {} Get your API key at: {}",
        style("ℹ").blue(),
        style("https://z.ai/subscribe").cyan()
    );

    let key: String = input("Enter your new Z.ai API key:")
        .validate(|input: &String| {
            if input.trim().is_empty() {
                Err("API key cannot be empty")
            } else {
                Ok(())
            }
        })
        .interact()?;

    // Save the API key
    config.set_secret("ZAI_API_KEY", &key)?;
    println!("  {} API key updated", style("✓").green());

    // Test the new API key
    println!("Testing configuration...");
    let model = config
        .get_param::<String>("GOOSE_MODEL")
        .unwrap_or_else(|_| "glm-4.6".to_string());
    match test_zai_config(&key, &model).await {
        Ok(_) => {
            println!("{} API key test successful!", style("✓").green());
        }
        Err(e) => {
            println!("{} API key test failed: {}", style("✗").red(), e);
            return Err(e);
        }
    }

    Ok(())
}

async fn test_zai_config(api_key: &str, model: &str) -> Result<()> {
    use goose::conversation::message::Message;
    use goose::providers::{create, providers};

    // Create a temporary config with the provided API key and model
    std::env::set_var("ZAI_API_KEY", api_key);

    // Get the zai provider
    let providers = providers().await;
    let zai_provider = providers
        .iter()
        .find(|(metadata, _)| metadata.name == "zai")
        .ok_or_else(|| anyhow::anyhow!("Z.ai provider not found"))?;

    let model_config = ModelConfig::new(model)?;
    let provider = create(&zai_provider.0.name, model_config).await?;

    // Test with a simple message
    let test_message = Message::user().with_text("Hello, can you respond with just 'OK'?");
    let _result = provider
        .complete("You are a helpful assistant.", &[test_message], &[])
        .await?;

    Ok(())
}
