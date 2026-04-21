import UIKit

final class WelcomeViewController: UITableViewController {
    init() {
        super.init(style: .insetGrouped)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("WelcomeViewController does not support Interface Builder")
    }

    override func viewDidLoad() {
        super.viewDidLoad()
        title = "Paws Examples"
        navigationItem.largeTitleDisplayMode = .always
        tableView.register(UITableViewCell.self, forCellReuseIdentifier: "cell")
        tableView.rowHeight = UITableView.automaticDimension
        tableView.estimatedRowHeight = 64
    }

    override func numberOfSections(in tableView: UITableView) -> Int {
        ExampleCatalog.sections.count
    }

    override func tableView(_ tableView: UITableView, numberOfRowsInSection section: Int) -> Int {
        ExampleCatalog.sections[section].entries.count
    }

    override func tableView(_ tableView: UITableView, titleForHeaderInSection section: Int) -> String? {
        ExampleCatalog.sections[section].title
    }

    override func tableView(_ tableView: UITableView, titleForFooterInSection section: Int) -> String? {
        ExampleCatalog.sections[section].footer
    }

    override func tableView(_ tableView: UITableView, cellForRowAt indexPath: IndexPath) -> UITableViewCell {
        let cell = tableView.dequeueReusableCell(withIdentifier: "cell", for: indexPath)
        let entry = ExampleCatalog.sections[indexPath.section].entries[indexPath.row]

        var config = UIListContentConfiguration.subtitleCell()
        config.text = entry.displayName
        config.secondaryText = entry.description
        config.image = UIImage(systemName: entry.symbolName)
        config.imageProperties.tintColor = .tintColor
        config.textProperties.font = .preferredFont(forTextStyle: .body)
        config.secondaryTextProperties.font = .preferredFont(forTextStyle: .footnote)
        config.secondaryTextProperties.color = .secondaryLabel
        config.secondaryTextProperties.numberOfLines = 2
        cell.contentConfiguration = config
        cell.accessoryType = .disclosureIndicator
        return cell
    }

    override func tableView(_ tableView: UITableView, didSelectRowAt indexPath: IndexPath) {
        tableView.deselectRow(at: indexPath, animated: true)
        let entry = ExampleCatalog.sections[indexPath.section].entries[indexPath.row]
        let runner = ExampleRunnerViewController(entry: entry)
        navigationController?.pushViewController(runner, animated: true)
    }
}
