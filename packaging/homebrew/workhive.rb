# Homebrew formula template for workhive.
#
# This file lives in the main repository as the source-of-truth template.
# To publish it, copy it into the tap repository (see packaging/homebrew/README.md)
# and replace every REPLACE_* placeholder with values from a published GitHub
# release. The sha256 values come from the *.tar.gz.sha256 assets uploaded by
# the release workflow.
class Workhive < Formula
  desc "Tmux-based development workspace manager for parallel AI coding"
  homepage "https://github.com/yboker/workhive"
  version "REPLACE_VERSION"
  license any_of: ["MIT", "Apache-2.0"]

  on_macos do
    on_arm do
      url "https://github.com/yboker/workhive/releases/download/vREPLACE_VERSION/workhive-REPLACE_VERSION-aarch64-apple-darwin.tar.gz"
      sha256 "REPLACE_SHA256_MACOS_ARM64"
    end
    on_intel do
      url "https://github.com/yboker/workhive/releases/download/vREPLACE_VERSION/workhive-REPLACE_VERSION-x86_64-apple-darwin.tar.gz"
      sha256 "REPLACE_SHA256_MACOS_X86_64"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/yboker/workhive/releases/download/vREPLACE_VERSION/workhive-REPLACE_VERSION-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "REPLACE_SHA256_LINUX_X86_64"
    end
  end

  depends_on "tmux"

  def install
    bin.install "workhive"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/workhive --version")
  end
end
