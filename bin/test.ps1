$here = $PSScriptRoot
$project_root = (Get-Item $here).Parent.FullName
$module_dir = "$project_root\target\debug"

emacs --version

$env:PROJECT_ROOT = $project_root
$env:MODULE_DIR = $module_dir

emacs --batch --directory "$module_dir" `
  -l ert `
  -l "$project_root\test-module\tests\main.el" `
  -f ert-run-tests-batch-and-exit
