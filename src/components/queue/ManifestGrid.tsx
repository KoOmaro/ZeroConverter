import React, { forwardRef, useRef, useState, useEffect } from "react";
import { VirtuosoGrid } from "react-virtuoso";
import { PendingFile } from "../../application/converterStore";
import { ManifestCard } from "./ManifestCard";

interface ManifestGridProps {
  files: PendingFile[];
  selectedId: string | null;
  onSelect: (id: string) => void;
  onRemove: (id: string) => void;
  onAddFiles: () => void;
}

// Custom List container for VirtuosoGrid — provides the CSS Grid layout
const GridList = forwardRef<HTMLDivElement, React.HTMLAttributes<HTMLDivElement>>(
  ({ style, children, ...props }, ref) => (
    <div
      ref={ref}
      {...props}
      style={{ ...style, paddingTop: 4, paddingRight: 24, paddingBottom: 16, paddingLeft: 24 }}
      className="grid grid-cols-[repeat(auto-fill,minmax(168px,1fr))] gap-3"
    >
      {children}
    </div>
  )
);
GridList.displayName = 'GridList';

// Custom Item wrapper
const GridItem = forwardRef<HTMLDivElement, React.HTMLAttributes<HTMLDivElement>>(
  ({ children, ...props }, ref) => (
    <div ref={ref} {...props}>
      {children}
    </div>
  )
);
GridItem.displayName = 'GridItem';

export const ManifestGrid: React.FC<ManifestGridProps> = ({
  files,
  selectedId,
  onSelect,
  onRemove,
  onAddFiles: _onAddFiles,
}) => {
  const containerRef = useRef<HTMLDivElement>(null);
  const [measuredHeight, setMeasuredHeight] = useState(0);

  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;

    const observer = new ResizeObserver((entries) => {
      for (const entry of entries) {
        const h = entry.contentRect.height;
        if (h > 0) setMeasuredHeight(h);
      }
    });
    observer.observe(el);
    return () => observer.disconnect();
  }, []);

  return (
    <div className="flex flex-col flex-1 min-h-0">
      <div ref={containerRef} className="flex-1 min-h-0 relative">
        {measuredHeight > 0 && (
          <VirtuosoGrid
            style={{ height: measuredHeight, width: '100%' }}
            data={files}
            computeItemKey={(_, file) => file.id}
            overscan={800}
            // Hide scrollbar entirely to prevent the jumping thumb annoyance
            
            components={{
              List: GridList as any,
              Item: GridItem as any,
            }}
            itemContent={(_, file) => (
              <ManifestCard
                key={file.id}
                file={file}
                isSelected={selectedId === file.id}
                onSelect={onSelect}
                onRemove={onRemove}
              />
            )}
          />
        )}
      </div>

      {/* Grid Footer */}
    </div>
  );
};
