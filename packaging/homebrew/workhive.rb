# Homebrew formula template for workhive
#
# To use this as a Homebrew tap:
# 1. Create a new repo: github.com/yboker/homebrew-workhive
# 2. Copy this file to Formula/workhive.rb in that repo
# 3. Update the sha256 hash (see instructions below)
# 4. Users install via: brew tap yboker/workhive && brew install workhive
#
# To calculate sha256:
#   curl -fsSL https://github.com/yboker/workhive/archive/refs/tags/v0.2.0.tar.gz | shasum -a 256

class Workhive < Formula
  desc "Cross-platform development workspace manager with Git Worktrees + tmux"
  homepage "https://github.com/yboker/workhive"
  url "https://github.com/yboker/workhive/archive/refs/tags/v0.2.0.tar.gz"
  sha256 "REPLACE_WITH_ACTUAL_SHA256"
  license "MIT"
  head "https://github.com/yboker/workhive.git", branch: "main"

  depends_on "bash"
  depends_on "tmux"
  depends_on "yq"
  depends_on "git"

  def install
    # lib/ installed to libexec (private, not exposed to PATH)
    libexec.install "lib"

    # Install shared resources
    libexec.install "configs" if (buildpath/"configs").exist?
    libexec.install "examples" if (buildpath/"examples").exist?

    # Patch entry script paths
    inreplace "bin/workhive",
      'ROOT_DIR="$(cd "$BIN_DIR/.." && pwd)"',
      "ROOT_DIR=\"#{libexec}\""

    inreplace "bin/workhive",
      'export LIB_DIR="$ROOT_DIR/lib"',
      "export LIB_DIR=\"#{libexec}/lib\""

    # Install main executable (symlinked to HOMEBREW_PREFIX/bin)
    bin.install "bin/workhive"

    # Install shell completions
    if (buildpath/"completions/workhive.bash").exist?
      bash_completion.install "completions/workhive.bash" => "workhive"
    end
    if (buildpath/"completions/workhive.zsh").exist?
      zsh_completion.install "completions/workhive.zsh" => "_workhive"
    end
    if (buildpath/"completions/workhive.fish").exist?
      fish_completion.install "completions/workhive.fish"
    end
  end

  test do
    assert_match "workhive", shell_output("#{bin}/workhive --version")
    assert_match "workhive", shell_output("#{bin}/workhive --help")
  end
end
