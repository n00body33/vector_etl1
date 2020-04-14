import React from 'react';

import Heading from '@theme/Heading';
import HighlightItem from '@theme/HighlightItem';
import Link from '@docusaurus/Link';

import classnames from 'classnames';
import GithubSlugger from 'github-slugger';
import humanizeString from 'humanize-string';
import pluralize from 'pluralize';

const AnchoredH2 = Heading('h2');
const AnchoredH3 = Heading('h3');

function normalizeItems(items) {
  return items.map(item => {
    if (item.content) {
      const {content: HighlightContents} = item;
      const {frontMatter, metadata} = HighlightContents;
      const {author_github: authorGithub, pr_numbers: prNumbers, release, title} = frontMatter;
      const {date: dateString, description, permalink, tags} = metadata;

      let map = {};
      map['authorGithub'] = authorGithub;
      map['dateString'] = dateString;
      map['description'] = description;
      map['permalink'] = permalink;
      map['prNumbers'] = prNumbers;
      map['release'] = release;
      map['tags'] = tags;
      map['title'] = title;
      return map
    } else {
      return item;
    }
  });
}

function Header({groupBy, group}) {
  switch(groupBy) {
    case 'release':
      return (
        <li className="header sticky">
          <Link to={`/releases/${group}/download/`}>{group}</Link>
        </li>
      );
      break;

    case 'type':
      const slugger = new GithubSlugger();

      return (
        <li className="header sticky">
          <AnchoredH3 id={slugger.slug(`${group}-highlights`)}>{pluralize(humanizeString(group))}</AnchoredH3>
        </li>
      );
      break;
  }
}

function HighlightItems({clean, groupBy, items, timeline}) {
  let defaultedGroupBy = groupBy || 'release';
  let normalizedItems = normalizeItems(items);
  let groupedItems = _.groupBy(normalizedItems, defaultedGroupBy);
  let groupKeys = timeline !== false ? Object.keys(groupedItems) : Object.keys(groupedItems).sort();

  return (
    <ul className={classnames('connected-list', {'connected-list--clean': clean === true, 'connected-list--timeline': timeline !== false})}>
      {groupKeys.map((group, idx) => {
        let groupItems = groupedItems[group];

        return (
          <>
            <Header groupBy={defaultedGroupBy} group={group} />
            {groupItems.map((highlight, idx) => {
              return <li key={idx}>
                <HighlightItem {...highlight} />
              </li>
            })}
          </>
        );
      })}
    </ul>
  );
}

export default HighlightItems;
