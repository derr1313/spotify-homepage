import React, { Fragment, useRef, useEffect, useState, CSSProperties } from 'react';
import * as R from 'ramda';
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome';
import { faPlay, faPause } from '@fortawesome/free-solid-svg-icons';
import HTMLEllipsis from 'react-lines-ellipsis/lib/html';
import ReactDOMServer from 'react-dom/server';
import { Link } from 'react-router-dom';

import { Timeframe } from 'src/types';
import { ANewTab, truncateWithElipsis, map } from 'src/util';
import { useUsername } from './store/selectors';
import './Cards.scss';

interface ImageBoxProps {
  imageSrc: string;
  imgAlt: string;
  linkTo?: string;
}

const ImageBox: React.FunctionComponent<ImageBoxProps> = ({
  imageSrc,
  imgAlt,
  children,
  linkTo,
}) => {
  const image = <img alt={imgAlt} src={imageSrc} className="image-container" />;

  return (
    <div className="image-box">
      <div className="track">
        {linkTo ? <Link to={linkTo}>{image}</Link> : image}

        <div className="image-box-content">{children}</div>
      </div>
    </div>
  );
};

interface TrackProps {
  title: string;
  artists: { name: string; uri: string; id: string }[];
  previewUrl: string;
  album: string;
  imageSrc: string;
  playing: string | false;
  setPlaying: (currentlyPlayingPreviewUrl: string | false) => void;
}

export const buildArtistStatsUrl = (username: string, artistId: string): string =>
  `/stats/${username}/artist/${artistId}/`;

const ArtistStatsLink: React.FC<{ artistId: string }> = ({ artistId, children }) => {
  const username = useUsername();
  if (!username) {
    return <>children</>;
  }

  return <Link to={buildArtistStatsUrl(username, artistId)}>{children}</Link>;
};

export const Track: React.FunctionComponent<TrackProps> = ({
  title,
  artists,
  previewUrl,
  album,
  imageSrc,
  playing,
  setPlaying,
}) => {
  const isPlaying = playing && playing === previewUrl;
  const audioTag = useRef<HTMLAudioElement | null>(null);

  useEffect(() => {
    const audioElem = audioTag.current;
    if (!audioElem) {
      return;
    }

    audioElem.volume = 0.2;
    if (!onended) {
      audioElem.onended = () => setPlaying(false);
    }

    if (isPlaying) {
      audioElem.play();
    } else {
      audioElem.pause();
    }
  });

  return (
    <ImageBox
      imgAlt={`Album art for ${title} on ${album} by ${artists.map(R.prop('name')).join(', ')}`}
      imageSrc={imageSrc}
    >
      <div className="card-data">
        <div>{truncateWithElipsis(title, 50)}</div>
        <span style={{ zIndex: 2 }}>
          {artists.map(({ name, id }, i) => {
            return (
              <Fragment key={name}>
                <ArtistStatsLink artistId={id}>{name}</ArtistStatsLink>
                {i !== artists.length - 1 ? ', ' : null}
              </Fragment>
            );
          })}
        </span>
        <audio preload="none" ref={audioTag} src={previewUrl} />
      </div>

      <div
        className="play-pause-button-wrapper"
        onClick={() => setPlaying(isPlaying ? false : previewUrl)}
      >
        {previewUrl ? <FontAwesomeIcon icon={isPlaying ? faPause : faPlay} /> : null}
      </div>
    </ImageBox>
  );
};

// TODO: Make this configurable, or better yet automatic based off of the user's top genres.
//       Not high-priority.
const DEFAULT_PREFERRED_GENRES = new Set([
  'vapor twitch',
  'vapor soul',
  'art pop',
  'indie pop',
  'indietronica',
  'folk-pop',
  'chillwave',
]);

interface ArtistProps {
  id: string;
  name: string;
  genres: string[];
  imageSrc: string;
  uri: string;
  preferredGenres?: Set<string>;
}

const Genre = ({ genre }: { genre: string }) => {
  const to = `http://everynoise.com/engenremap-${genre.replace(/ /g, '')}.html`;
  return <ANewTab to={to} text={genre} style={{ color: 'white', fontSize: 11 }} />;
};

export const Artist = ({
  id,
  name,
  genres,
  imageSrc,
  preferredGenres = DEFAULT_PREFERRED_GENRES,
}: ArtistProps) => {
  const username = useUsername();
  // Make sure that preferred genres show up and aren't trimmed off
  const [preferred, other] = R.partition(genre => preferredGenres.has(genre), genres);
  const trimmedGenres = [...preferred, ...other].slice(0, 6);

  return (
    <ImageBox
      imgAlt={name}
      imageSrc={imageSrc}
      linkTo={map(username, username => buildArtistStatsUrl(username, id)) || undefined}
    >
      <div className="card-data">
        <div>
          <ArtistStatsLink artistId={id}>{name}</ArtistStatsLink>
        </div>
        <HTMLEllipsis
          maxLine={3}
          basedOn="words"
          trimRight={false}
          unsafeHTML={ReactDOMServer.renderToString(
            <div style={{ lineHeight: '1em' }}>
              {trimmedGenres.map((genre, i) => (
                <Fragment key={genre}>
                  <Genre genre={genre} />
                  {i !== trimmedGenres.length - 1 ? ', ' : null}
                </Fragment>
              ))}
            </div>
          )}
        />
      </div>
    </ImageBox>
  );
};

const TIMEFRAMES: Timeframe[] = ['short', 'medium', 'long'];

interface TimeframeSelectorProps {
  timeframe: Timeframe;
  setTimeframe: (newTimeframe: Timeframe) => void;
}

export const TimeframeSelector: React.FunctionComponent<TimeframeSelectorProps> = ({
  timeframe,
  setTimeframe,
}) => (
  <div className="timeframe-selector">
    Timeframe:{' '}
    {TIMEFRAMES.map((frame, i, frames) => (
      <Fragment key={frame}>
        <span
          style={{
            textDecoration: 'underline',
            ...(frame === timeframe ? { fontWeight: 'bold', fontSize: 22 } : { cursor: 'pointer' }),
          }}
          onClick={() => setTimeframe(frame)}
        >
          {frame}
        </span>
        {i !== frames.length - 1 ? ' \u2022 ' : null}
      </Fragment>
    ))}
  </div>
);

interface ImageBoxGridProps {
  renderItem: (i: number, timeframe: Timeframe) => React.ReactNode;
  initialItems: number;
  maxItems: number;
  title: string;
}

export const ImageBoxGrid: React.FunctionComponent<ImageBoxGridProps> = ({
  renderItem,
  initialItems,
  maxItems,
  title,
}) => {
  const [timeframe, setTimeframe] = useState<Timeframe>('short');
  const [isExpanded, setIsExpanded] = useState(false);
  const itemCount = isExpanded ? maxItems : initialItems;

  return (
    <>
      <h3 className="image-box-grid-title">{title}</h3>
      <TimeframeSelector timeframe={timeframe} setTimeframe={setTimeframe} />
      <div className="image-box-grid">
        {R.times(R.identity, itemCount).map(i => renderItem(i, timeframe))}
      </div>

      {!isExpanded ? (
        <div onClick={() => setIsExpanded(true)} className="show-more">
          Show More
        </div>
      ) : null}
    </>
  );
};
