import React from "react";

export type IconKind = 'image' | 'audio' | 'video' | 'pdf' | 'word' | 'spreadsheet' | 'presentation' | 'document' | 'unknown' | 'unsupported';

interface FileTypeIconProps {
  kind: IconKind;
  className?: string;
}

export const FileTypeIcon: React.FC<FileTypeIconProps> = ({ kind, className = "w-6 h-6" }) => {
  const getIcon = () => {
    switch (kind) {
      case 'image':
        return (
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" className={className}>
            <rect x="3" y="3" width="18" height="18" rx="2" ry="2" />
            <circle cx="8.5" cy="8.5" r="1.5" />
            <polyline points="21 15 16 10 5 21" />
          </svg>
        );
      case 'audio':
        return (
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" className={className}>
            <path d="M9 18V5l12-2v13" />
            <circle cx="6" cy="18" r="3" />
            <circle cx="18" cy="16" r="3" />
          </svg>
        );
      case 'video':
        return (
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" className={className}>
            <rect x="2" y="5" width="14" height="14" rx="2" ry="2" />
            <polygon points="22 7 16 12 22 17 22 7" />
          </svg>
        );
      case 'pdf':
        return (
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" className={className}>
            <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
            <polyline points="14 2 14 8 20 8" />
            <path d="M9 15h3a1 1 0 0 1 0 2H9M15 15h1a1 1 0 0 1 1 1v2a1 1 0 0 1-1 1h-1M12 12v3" />
          </svg>
        );
      case 'word':
        return (
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" className={className}>
            <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
            <polyline points="14 2 14 8 20 8" />
            <line x1="9" y1="13" x2="15" y2="13" />
            <line x1="9" y1="17" x2="15" y2="17" />
          </svg>
        );
      case 'spreadsheet':
        return (
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" className={className}>
            <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
            <polyline points="14 2 14 8 20 8" />
            <line x1="10" y1="13" x2="10" y2="17" />
            <line x1="14" y1="13" x2="14" y2="17" />
            <line x1="8" y1="15" x2="16" y2="15" />
          </svg>
        );
      case 'unsupported':
      case 'unknown':
      default:
        return (
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" className={className}>
            <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
            <line x1="12" y1="18" x2="12.01" y2="18" />
            <line x1="12" y1="13" x2="12" y2="13" />
          </svg>
        );
    }
  };

  return <div className="text-secondary">{getIcon()}</div>;
};
