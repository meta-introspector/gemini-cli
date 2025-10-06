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

## 4. Rebasing onto Upstream Latest

To keep your local branch updated with the latest changes from the main Gemini CLI repository, you should regularly rebase onto `upstream/main`.

1.  **Fetch latest changes from upstream:**

    ```bash
    git fetch upstream
    ```

2.  **Rebase your current branch:**

    ```bash
    git rebase upstream/main
    ```

    Resolve any conflicts that may arise during the rebase.

## 5. Building and Testing with Nix

After setting up your development environment and rebasing, you can build and test the Gemini CLI using Nix.

1.  **Build the project using Nix:**

    ```bash
    nix build .#gemini-cli
    ```

    This command will build the `gemini-cli` package defined in the flake. The built output will be symlinked to `./result` in your current directory.

2.  **Run the built Gemini CLI:**

    You can execute the newly built CLI directly from the `result` symlink:

    ```bash
    ./result/bin/gemini --version
    ```

    This will output the version of the built Gemini CLI, confirming a successful build.

3.  **Install Node.js dependencies (if needed for development tasks outside Nix build):**

    ```bash
    npm install
    ```

4.  **Run tests:**

    ```bash
    npm test
    ```

5.  **Build the project (using npm, if not using Nix build directly):**

    ```bash
    npm run build
    ```

## 6. Contributing with Nix

When contributing, ensure your changes are compatible with the Nix environment.

*   **Flake Inputs:** Always use `github:meta-introspector` URLs for flake inputs, referencing specific branches (e.g., `ref=feature/your-feature-branch`).
*   **Purity:** Strive for Nix purity in your builds. All inputs should come from the Nix store, and outputs should go to the Nix store.
*   **Testing:** Before committing, run `npm run preflight` to ensure all checks pass within the Nix development environment.

## 7. Updating Flake Inputs

If you need to update any flake inputs (e.g., `nixpkgs`, `flake-utils`), use `nix flake update`.

```bash
nix flake update
```

Remember to commit the updated `flake.lock` file after updating inputs.
