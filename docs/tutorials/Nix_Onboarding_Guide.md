# Nix Onboarding Guide for Gemini CLI

This guide provides detailed instructions for setting up and using the Gemini CLI with Nix, ensuring a reproducible development environment and streamlined contribution workflow.

## 1. Prerequisites

Before you begin, ensure you have Nix installed on your system. If not, follow the official Nix installation guide: [https://nixos.org/download.html](https://nixos.org/download.html)

## 2. Installing Gemini CLI with Nix

To install the Gemini CLI using its Nix flake, run the following command:

```bash
nix profile install github:meta-introspector/gemini-cli?ref=feature/working-gemini-cli-nix-store
```

This command adds the `gemini-cli` package to your Nix profile, making it available in your shell.

## 3. Setting up a Development Environment

For development, it's recommended to use the `devShell` provided by the flake. This sets up all necessary development tools and dependencies.

1.  **Clone the repository:**

    ```bash
    git clone https://github.com/meta-introspector/gemini-cli.git
    cd gemini-cli
    ```

2.  **Enter the development shell:**

    ```bash
    nix develop
    ```

    This command will drop you into a shell environment where all development dependencies (Node.js, npm, etc.) are available.

## 4. Building and Testing

Inside the `nix develop` shell:

1.  **Install Node.js dependencies:**

    ```bash
    npm install
    ```

2.  **Run tests:**

    ```bash
    npm test
    ```

3.  **Build the project:**

    ```bash
    npm run build
    ```

## 5. Contributing with Nix

When contributing, ensure your changes are compatible with the Nix environment.

*   **Flake Inputs:** Always use `github:meta-introspector` URLs for flake inputs, referencing specific branches (e.g., `ref=feature/your-feature-branch`).
*   **Purity:** Strive for Nix purity in your builds. All inputs should come from the Nix store, and outputs should go to the Nix store.
*   **Testing:** Before committing, run `npm run preflight` to ensure all checks pass within the Nix development environment.

## 6. Updating Flake Inputs

If you need to update any flake inputs (e.g., `nixpkgs`, `flake-utils`), use `nix flake update`.

```bash
nix flake update
```

Remember to commit the updated `flake.lock` file after updating inputs.
